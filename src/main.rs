use anyhow::Result;
use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor, SetBackgroundColor},
    terminal::{self, Clear, ClearType},
};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    /// PDF file to process
    file: PathBuf,
    
    /// Page number to extract (default: 1)
    #[arg(short, long, default_value_t = 1)]
    page: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ActivePane {
    Image,  // Left pane - PDF image controls
    Text,   // Right pane - text editing
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LayoutMode {
    Spatial,    // Perfect tables, some word spacing issues
    Sequential, // Perfect text flow, table alignment issues
}

#[derive(Debug, Clone)]
struct AltoElement {
    _id: String,
    content: String,
    hpos: f32,
    vpos: f32,
    _width: f32,
    _height: f32,
    // Screen position (calculated from PDF coordinates)
    _screen_x: u16,
    _screen_y: u16,
    element_type: ElementType,
}

#[derive(Debug, Clone, PartialEq)]
enum ElementType {
    Text,
    Space,
}

impl AltoElement {
    fn new(id: String, content: String, hpos: f32, vpos: f32, width: f32, height: f32) -> Self {
        // Improved coordinate mapping - preserve relative positioning
        // Use page-relative coordinates instead of fixed division
        let screen_x = (hpos * 0.1) as u16; // Better scaling factor
        let screen_y = (vpos * 0.08) as u16; // Preserve vertical relationships
        
        let element_type = if content == " " {
            ElementType::Space
        } else {
            ElementType::Text
        };
        
        Self {
            _id: id,
            content,
            hpos,
            vpos,
            _width: width,
            _height: height,
            _screen_x: screen_x,
            _screen_y: screen_y,
            element_type,
        }
    }
}

struct WysiwygEditor {
    elements: Vec<AltoElement>,
    pdf_path: PathBuf,
    current_page: u32,
    terminal_width: u16,
    terminal_height: u16,
    // Text buffer for continuous editing
    text_buffer: String,
    cursor_x: u16,
    cursor_y: u16,
    // A-B comparison mode
    ab_mode: bool,
    pdf_image_rendered: bool,
    // Dark mode for PDF display
    dark_mode: bool,
    // Block text selection (always on)
    selection_start_x: u16,
    selection_start_y: u16,
    selection_end_x: u16,
    selection_end_y: u16,
    is_selecting: bool,
    // Clipboard for copy/paste
    clipboard: Vec<String>,
    // File picker
    file_picker_open: bool,
    file_list: Vec<PathBuf>,
    file_picker_selected: usize,
    file_picker_path: PathBuf,
    // Zoom controls
    zoom_level: u16, // Resolution multiplier (100, 150, 200, 300, etc.)
    // Pane focus (for A-B mode)
    active_pane: ActivePane,
    // Image viewport panning
    pan_offset_x: i16,
    pan_offset_y: i16,
    // Undo/redo system
    history: Vec<String>,
    history_index: isize, // -1 means at latest, 0+ means historical position
    // Layout mode
    layout_mode: LayoutMode,
    // Performance: dirty tracking
    text_buffer_dirty: bool,
    last_layout_mode: LayoutMode,
    // Scroll offset for text mode
    scroll_offset: usize,
    // Total pages in PDF
    total_pages: u32,
}

impl WysiwygEditor {
    fn new(pdf_path: PathBuf, page: u32) -> Result<Self> {
        let (width, height) = terminal::size()?;
        let file_picker_path = pdf_path.parent().unwrap_or_else(|| std::path::Path::new(".")).to_path_buf();
        let mut editor = Self {
            elements: Vec::new(),
            pdf_path,
            current_page: page,
            terminal_width: width,
            terminal_height: height,
            text_buffer: String::new(),
            cursor_x: 0,
            cursor_y: 0,
            ab_mode: false,
            pdf_image_rendered: false,
            dark_mode: true, // Default to dark mode
            selection_start_x: 0,
            selection_start_y: 0,
            selection_end_x: 0,
            selection_end_y: 0,
            is_selecting: false,
            clipboard: Vec::new(),
            file_picker_open: false,
            file_list: Vec::new(),
            file_picker_selected: 0,
            file_picker_path,
            zoom_level: 100, // Start at 100 DPI
            active_pane: ActivePane::Text, // Start with text editing focused
            pan_offset_x: 0,
            pan_offset_y: 0,
            history: Vec::new(),
            history_index: -1,
            layout_mode: LayoutMode::Spatial, // Start with good tables
            text_buffer_dirty: true, // Needs initial build
            last_layout_mode: LayoutMode::Spatial,
            scroll_offset: 0,
            total_pages: 1, // Will be updated after loading
        };
        
        editor.load_page()?;
        // Save initial state to history
        editor.save_to_history();
        Ok(editor)
    }
    
    fn load_page(&mut self) -> Result<()> {
        // Update total pages when loading
        self.update_total_pages()?;
        self.elements = self.extract_alto_elements()?;
        self.text_buffer_dirty = true; // New elements require rebuild
        self.scroll_offset = 0; // Reset scroll when loading new page
        self.rebuild_text_buffer();
        Ok(())
    }
    
    fn update_total_pages(&mut self) -> Result<()> {
        let output = std::process::Command::new("pdfinfo")
            .arg(&self.pdf_path)
            .output()?;
            
        if output.status.success() {
            let info = std::str::from_utf8(&output.stdout).unwrap_or("");
            for line in info.lines() {
                if line.starts_with("Pages:") {
                    if let Some(pages_str) = line.split_whitespace().nth(1) {
                        if let Ok(pages) = pages_str.parse::<u32>() {
                            self.total_pages = pages;
                            return Ok(());
                        }
                    }
                }
            }
        }
        
        // Fallback: assume single page if pdfinfo fails
        self.total_pages = 1;
        Ok(())
    }
    
    fn rebuild_text_buffer(&mut self) {
        if self.elements.is_empty() {
            self.text_buffer = String::new();
            return;
        }
        
        // Skip rebuild if nothing changed (major performance win)
        if !self.text_buffer_dirty && self.layout_mode == self.last_layout_mode {
            return;
        }
        
        match self.layout_mode {
            LayoutMode::Spatial => self.rebuild_spatial_layout(),
            LayoutMode::Sequential => self.rebuild_sequential_layout(),
        }
        
        // Mark as clean and update last mode
        self.text_buffer_dirty = false;
        self.last_layout_mode = self.layout_mode;
    }
    
    fn rebuild_spatial_layout(&mut self) {
        let (min_hpos, _max_hpos, min_vpos, _max_vpos, page_width, page_height) = self.calculate_page_bounds();
        
        // Dynamic grid sizing based on actual content bounds
        let terminal_width = ((page_width / 6.0) as usize + 20).min(200).max(80); // Adaptive width
        let terminal_height = ((page_height / 12.0) as usize + 10).min(100).max(30); // Adaptive height
        
        let mut grid = vec![' '; terminal_width * terminal_height]; // Flat grid for speed
        

        // Map each element to terminal grid using precise coordinates (RESTORE HIGH WATERMARK)
        for element in &self.elements {
            // Don't skip spaces anymore - they're important for layout
            let content = if element.content == " " {
                " " // Preserve space elements as-is
            } else {
                element.content.trim()
            };
            
            if content.is_empty() && element.content != " " {
                continue; // Skip only truly empty content, not spaces
            }
            
            // Fast integer coordinate mapping
            let x = if page_width > 0.0 {
                (((element.hpos - min_hpos) * (terminal_width - 10) as f32) / page_width) as usize
            } else { 0 }.min(terminal_width.saturating_sub(1));
            
            let y = if page_height > 0.0 {
                (((element.vpos - min_vpos) * (terminal_height - 5) as f32) / page_height) as usize
            } else { 0 }.min(terminal_height.saturating_sub(1));
            
            // Place text in grid, handling overlaps
            let content = if element.content == " " {
                " " // Preserve space elements as-is
            } else {
                element.content.trim()
            };
            
            if y < terminal_height && x < terminal_width && !content.is_empty() {
                let grid_idx = y * terminal_width + x;
                if grid_idx < grid.len() {
                    // For space elements, just place a single space
                    if element.content == " " {
                        if grid[grid_idx] == ' ' { // Only place space if position is empty
                            grid[grid_idx] = ' ';
                        }
                    } else {
                        // For text elements, place character by character
                        for (i, ch) in content.char_indices() {
                            let pos_x = x + i;
                            if pos_x < terminal_width {
                                let pos_idx = y * terminal_width + pos_x;
                                if pos_idx < grid.len() {
                                    // Only overwrite spaces or if element has priority (inlined)
                                    let has_priority = grid[pos_idx] == ' ' || 
                                        element.content.chars().any(|c| c.is_numeric() || c == '$' || c == '%');
                                    if has_priority {
                                        grid[pos_idx] = ch;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Convert flat grid to text efficiently  
        let buffer_lines: Vec<String> = grid.chunks(terminal_width)
            .map(|row| row.iter().collect::<String>().trim_end_matches(' ').to_owned())
            .filter(|line| !line.trim().is_empty())
            .collect();
        self.text_buffer = buffer_lines.join("\n");
    }
    
    fn calculate_page_bounds(&self) -> (f32, f32, f32, f32, f32, f32) {
        if self.elements.is_empty() {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        }
        
        // Fast single-pass bounds calculation
        let mut min_hpos = f32::INFINITY;
        let mut max_hpos = f32::NEG_INFINITY;
        let mut min_vpos = f32::INFINITY;
        let mut max_vpos = f32::NEG_INFINITY;
        
        for element in &self.elements {
            min_hpos = min_hpos.min(element.hpos);
            max_hpos = max_hpos.max(element.hpos + element._width);
            min_vpos = min_vpos.min(element.vpos);
            max_vpos = max_vpos.max(element.vpos + element._height);
        }
        
        let page_width = max_hpos - min_hpos;
        let page_height = max_vpos - min_vpos;
        (min_hpos, max_hpos, min_vpos, max_vpos, page_width, page_height)
    }
    
    fn rebuild_sequential_layout(&mut self) {
        let (min_hpos, _max_hpos, min_vpos, _max_vpos, page_width, _page_height) = self.calculate_page_bounds();
        let page_center = min_hpos + (page_width / 2.0);
        
        // Group elements by line with better clustering
        let mut lines_map = std::collections::HashMap::new();
        for element in &self.elements {
            let line_y = ((element.vpos - min_vpos) / 8.0) as usize; // Tighter line grouping
            lines_map.entry(line_y).or_insert_with(Vec::new).push(element.clone());
        }
        
        let mut output_lines = Vec::new();
        let char_width = 6.0;
        let terminal_width = 80;
        let mut last_vpos = 0.0;
        
        for y in 0..60 {
            if let Some(mut line_elements) = lines_map.remove(&y) {
                // Sort by horizontal position
                line_elements.sort_by(|a, b| a.hpos.partial_cmp(&b.hpos).unwrap_or(std::cmp::Ordering::Equal));
                
                // Check for paragraph breaks based on vertical gap
                let current_vpos = line_elements.first().map(|e| e.vpos).unwrap_or(0.0);
                let vpos_gap = current_vpos - last_vpos;
                
                // Add paragraph breaks for large gaps
                if last_vpos > 0.0 && vpos_gap > 20.0 {
                    output_lines.push(String::new()); // Empty line for paragraph break
                }
                
                // Build line with proper spacing
                let mut line_output = String::new();
                for element in &line_elements {
                    match element.element_type {
                        ElementType::Text => {
                            line_output.push_str(element.content.trim());
                        }
                        ElementType::Space => {
                            let space_count = (element._width / char_width).round().max(1.0) as usize;
                            for _ in 0..space_count.min(3) { line_output.push(' '); }
                        }
                    }
                }
                
                // Check if line should be centered (like titles, table headers)
                if self.is_line_centered_sequential(&line_elements, page_center, page_width) {
                    let padding = ((terminal_width as i32 - line_output.len() as i32) / 2).max(0) as usize;
                    let mut padded = String::with_capacity(padding + line_output.len());
                    padded.push_str(&" ".repeat(padding));
                    padded.push_str(&line_output);
                    line_output = padded;
                }
                
                output_lines.push(line_output);
                last_vpos = current_vpos;
            }
        }
        
        // Remove trailing empty lines
        while output_lines.last() == Some(&String::new()) {
            output_lines.pop();
        }
        
        self.text_buffer = output_lines.join("\n");
    }
    
    fn is_line_centered_sequential(&self, line: &[AltoElement], page_center: f32, page_width: f32) -> bool {
        if line.is_empty() {
            return false;
        }
        
        // Calculate the center of this line's text
        let line_start = line.iter().map(|e| e.hpos).fold(f32::INFINITY, f32::min);
        let line_end = line.iter().map(|e| e.hpos + e._width).fold(f32::NEG_INFINITY, f32::max);
        let line_center = line_start + ((line_end - line_start) / 2.0);
        
        // Consider it centered if within 15% of page center
        let tolerance = page_width * 0.15;
        let distance_from_center = (line_center - page_center).abs();
        
        // Also check if line is significantly shorter than full width (typical for centered content)
        let line_width = line_end - line_start;
        let is_short_line = line_width < (page_width * 0.7);
        
        distance_from_center < tolerance && is_short_line
    }
    
    fn toggle_layout_mode(&mut self) -> Result<()> {
        self.layout_mode = match self.layout_mode {
            LayoutMode::Spatial => LayoutMode::Sequential,
            LayoutMode::Sequential => LayoutMode::Spatial,
        };
        
        // Mark as dirty and rebuild with new layout mode
        self.text_buffer_dirty = true;
        self.scroll_offset = 0; // Reset scroll when changing modes
        self.rebuild_text_buffer();
        
        Ok(())
    }
    
    
    
    
    fn extract_alto_elements(&self) -> Result<Vec<AltoElement>> {
        let output = std::process::Command::new("pdfalto")
            .args([
                "-f", &self.current_page.to_string(),
                "-l", &self.current_page.to_string(),
                "-noImage", "-noLineNumbers",
            ])
            .arg(&self.pdf_path)
            .arg("/dev/stdout")
            .output()?;
            
        if !output.status.success() {
            return Ok(vec![]);
        }
        
        let xml_data = std::str::from_utf8(&output.stdout).unwrap_or(""); // Faster than lossy conversion
        Ok(self.parse_alto_xml(&xml_data))
    }
    
    fn parse_alto_xml(&self, xml: &str) -> Vec<AltoElement> {
        use quick_xml::{Reader, events::Event};
        
        let mut elements = Vec::new();
        let mut reader = Reader::from_str(xml);
        let mut buf = Vec::new();
        let mut element_id = 0;
        
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.name().as_ref() == b"String" {
                        // Parse text elements
                        let mut content = String::new();
                        let mut hpos = 0.0;
                        let mut vpos = 0.0;
                        let mut width = 0.0;
                        let mut height = 10.0;
                        
                        for attr in e.attributes().with_checks(false) {
                            if let Ok(attr) = attr {
                                let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                                let value = std::str::from_utf8(&attr.value).unwrap_or("");
                                
                                match key.as_ref() {
                                    "CONTENT" => content = value.to_owned(),
                                    "HPOS" => hpos = value.parse().unwrap_or(0.0),
                                    "VPOS" => vpos = value.parse().unwrap_or(0.0),
                                    "WIDTH" => width = value.parse().unwrap_or(0.0),
                                    "HEIGHT" => height = value.parse().unwrap_or(10.0),
                                    _ => {}
                                }
                            }
                        }
                        
                        if !content.is_empty() {
                            elements.push(AltoElement::new(
                                format!("elem_{}", element_id),
                                content,
                                hpos,
                                vpos,
                                width,
                                height,
                            ));
                            element_id += 1;
                        }
                    } else if e.name().as_ref() == b"SP" {
                        // Parse space elements - these are the missing word boundaries!
                        let mut hpos = 0.0;
                        let mut vpos = 0.0;
                        let mut width = 3.0; // Default space width
                        let height = 10.0;
                        
                        for attr in e.attributes().with_checks(false) {
                            if let Ok(attr) = attr {
                                let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                                let value = std::str::from_utf8(&attr.value).unwrap_or("");
                                
                                match key.as_ref() {
                                    "HPOS" => hpos = value.parse().unwrap_or(0.0),
                                    "VPOS" => vpos = value.parse().unwrap_or(0.0),
                                    "WIDTH" => width = value.parse().unwrap_or(3.0),
                                    _ => {}
                                }
                            }
                        }
                        
                        // Add space as an element
                        elements.push(AltoElement::new(
                            format!("space_{}", element_id),
                            " ".to_owned(),
                            hpos,
                            vpos,
                            width,
                            height,
                        ));
                        element_id += 1;
                    }
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
            buf.clear();
        }
        
        elements
    }
    
    fn render(&self) -> Result<()> {
        // If file picker is open, render it instead of normal content
        if self.file_picker_open {
            self.render_file_picker()?;
            return Ok(());
        }
        
        // Minimal clearing to reduce flashing
        if !self.ab_mode {
            execute!(io::stdout(), Clear(ClearType::All), cursor::MoveTo(0, 0))?;
        }
        // In A-B mode, don't clear at all - just update what's needed
        
        if self.ab_mode {
            self.render_split_screen()?;
        } else {
            self.render_text_only()?;
        }
        
        // Status line with layout mode and pane info
        let mode_indicator = match self.layout_mode {
            LayoutMode::Spatial => "SPATIAL",
            LayoutMode::Sequential => "FLOW",
        };
        
        let pane_indicator = if self.ab_mode {
            match self.active_pane {
                ActivePane::Image => " - [IMAGE]",
                ActivePane::Text => " - [TEXT]",
            }
        } else {
            ""
        };
        
        let scroll_info = if !self.ab_mode && self.layout_mode == LayoutMode::Sequential {
            let total_lines = self.text_buffer.lines().count();
            let visible_lines = (self.terminal_height - 1) as usize;
            if total_lines > visible_lines {
                format!(" - Line {}/{}", self.scroll_offset + 1, total_lines)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        
        let status_text = format!("{} - Page {}/{} - {}% - {} (Ctrl+L){}{}", 
            self.pdf_path.file_stem().unwrap_or_default().to_string_lossy(),
            self.current_page,
            self.total_pages,
            self.zoom_level,
            mode_indicator,
            pane_indicator,
            scroll_info);
        
        execute!(
            io::stdout(),
            cursor::MoveTo(0, self.terminal_height - 1),
            SetForegroundColor(Color::DarkGrey),
            Print(status_text),
            ResetColor
        )?;
        
        // Position cursor only in text mode
        if !self.ab_mode {
            execute!(
                io::stdout(),
                cursor::MoveTo(self.cursor_x, self.cursor_y),
                cursor::Show
            )?;
        } else {
            // Hide cursor completely in A-B mode
            execute!(io::stdout(), cursor::Hide)?;
        }
        
        io::stdout().flush()?;
        Ok(())
    }
    
    fn render_text_only(&self) -> Result<()> {
        // Display the text buffer as continuous text (full width)
        // Calculate how many lines we can show (leave room for status line)
        let available_lines = (self.terminal_height - 1) as usize;
        let total_lines: Vec<&str> = self.text_buffer.lines().collect();
        
        // Clear the text area
        for y in 0..available_lines {
            execute!(io::stdout(), cursor::MoveTo(0, y as u16), Print(" ".repeat(self.terminal_width as usize)))?;
        }
        
        // Render visible lines starting from scroll_offset
        for (screen_line, text_line_index) in (self.scroll_offset..).enumerate() {
            if screen_line >= available_lines {
                break;
            }
            
            if let Some(line) = total_lines.get(text_line_index) {
                execute!(io::stdout(), cursor::MoveTo(0, screen_line as u16))?;
                self.render_line_with_selection(line, screen_line as u16, 0)?;
            }
        }
        Ok(())
    }
    
    fn render_split_screen(&self) -> Result<()> {
        let split_x = self.terminal_width / 2;
        
        // Left side: PDF image area 
        if !self.pdf_image_rendered {
            execute!(
                io::stdout(),
                cursor::MoveTo(2, 2),
                SetForegroundColor(Color::Cyan),
                Print("Loading PDF image..."),
                ResetColor,
                cursor::MoveTo(2, 4),
                SetForegroundColor(Color::Yellow),
                Print(format!("Page: {}", self.current_page)),
                ResetColor,
                cursor::MoveTo(2, 5),
                SetForegroundColor(Color::Yellow),
                Print(format!("File: {}", self.pdf_path.display())),
                ResetColor
            )?;
        } else {
            // Don't overwrite the image area - it should be displayed
            // The image is rendered at position 0,0 and takes up the left panel
            // We don't print anything over it
        }
        
        // Clean vertical separator with subtle focus indication
        for y in 0..(self.terminal_height - 1) {
            let separator_color = match self.active_pane {
                ActivePane::Image => Color::Blue,    // Subtle blue for image focus
                ActivePane::Text => Color::DarkGrey, // Muted for text focus
            };
            
            execute!(
                io::stdout(),
                cursor::MoveTo(split_x, y),
                SetForegroundColor(separator_color),
                Print("│"),
                ResetColor
            )?;
        }
        
        // Right side: Text extraction with clean black background
        let right_panel_width = (self.terminal_width - split_x - 1) as usize;
        
        // Clear right panel with black background
        for y in 0..(self.terminal_height - 1) {
            execute!(
                io::stdout(),
                cursor::MoveTo(split_x + 1, y),
                Print(" ".repeat(right_panel_width))
            )?;
        }
        
        // Render text on clean background
        for (i, line) in self.text_buffer.lines().enumerate() {
            if i < (self.terminal_height - 2) as usize {
                let trimmed_line = if line.len() > right_panel_width {
                    &line[0..right_panel_width]
                } else {
                    line
                };
                execute!(io::stdout(), cursor::MoveTo(split_x + 1, i as u16))?;
                self.render_line_with_selection(trimmed_line, i as u16, split_x + 1)?;
            }
        }
        
        Ok(())
    }
    
    fn render_line_with_selection(&self, line: &str, row: u16, col_offset: u16) -> Result<()> {
        if !self.is_selecting {
            // No selection, just print normally
            execute!(io::stdout(), Print(line))?;
            return Ok(());
        }
        
        
        // Calculate selection bounds
        let sel_start_x = self.selection_start_x.saturating_sub(col_offset);
        let sel_start_y = self.selection_start_y;
        let sel_end_x = self.selection_end_x.saturating_sub(col_offset);
        let sel_end_y = self.selection_end_y;
        
        // Normalize selection bounds (ensure start <= end)
        let (norm_start_y, norm_end_y) = if sel_start_y <= sel_end_y {
            (sel_start_y, sel_end_y)
        } else {
            (sel_end_y, sel_start_y)
        };
        
        let (norm_start_x, norm_end_x) = if sel_start_x <= sel_end_x {
            (sel_start_x, sel_end_x)
        } else {
            (sel_end_x, sel_start_x)
        };
        
        // Check if this row is within selection
        if row >= norm_start_y && row <= norm_end_y {
            let line_chars: Vec<char> = line.chars().collect();
            let selection_width = (norm_end_x - norm_start_x + 1) as usize;
            
            // Build the entire line in memory to reduce flicker
            let mut rendered_line = String::new();
            
            // Add pre-selection content
            for i in 0..norm_start_x as usize {
                if i < line_chars.len() {
                    rendered_line.push(line_chars[i]);
                } else {
                    rendered_line.push(' ');
                }
            }
            
            
            // Add selection content (will be styled separately)
            let mut selection_content = String::new();
            for i in 0..selection_width {
                let char_index = norm_start_x as usize + i;
                if char_index < line_chars.len() {
                    selection_content.push(line_chars[char_index]);
                } else {
                    selection_content.push(' ');
                }
            }
            
            // Add post-selection content
            let mut post_selection = String::new();
            for i in (norm_end_x + 1) as usize..line_chars.len() {
                post_selection.push(line_chars[i]);
            }
            
            // Render in one go to reduce flicker
            execute!(io::stdout(), Print(&rendered_line))?;
            execute!(io::stdout(), SetBackgroundColor(Color::Blue), SetForegroundColor(Color::White), Print(&selection_content), ResetColor)?;
            execute!(io::stdout(), Print(&post_selection))?;
        } else {
            // Row is not in selection - normal rendering
            execute!(io::stdout(), Print(line))?;
        }
        
        Ok(())
    }
    
    fn render_pdf_image(&mut self) -> Result<()> {
        if !self.ab_mode || self.pdf_image_rendered {
            return Ok(());
        }
        
        // Ultra-fast file operations - PPM for speed
        let output_file = format!("/tmp/chonker_kitty_{}.ppm", self.current_page);
        let use_temp = self.dark_mode;
        
        let (temp_file, final_file) = if use_temp {
            (format!("/tmp/chonker_temp_{}.ppm", self.current_page), 
             format!("/tmp/chonker_kitty_{}.png", self.current_page)) // Convert to PNG for dark mode
        } else {
            (output_file.clone(), output_file.clone())
        };
        
        // Clean up only what we need
        let _ = std::fs::remove_file(&final_file);
        if use_temp { let _ = std::fs::remove_file(&temp_file); }
        
        // Log to file instead of terminal
        let _ = std::fs::write("/tmp/chonker95_debug.log", 
            format!("Rendering PDF at {}% zoom ({}dpi) for page {}\n", self.zoom_level, self.zoom_level + 50, self.current_page));
        
        // Minimal ghostscript for maximum speed
        let target_dpi = (self.zoom_level as f32 * 0.7) as u16;
        let result = std::process::Command::new("gs")
            .args([
                "-dNOPAUSE", "-dBATCH", "-dQUIET", "-dNOSAFER",
                "-sDEVICE=ppmraw", // Raw PPM is fastest to generate
                &format!("-dFirstPage={}", self.current_page),
                &format!("-dLastPage={}", self.current_page),
                &format!("-r{}", target_dpi),
                &format!("-sOutputFile={}", temp_file),
            ])
            .arg(&self.pdf_path)
            .output()?;
            
        if !result.status.success() {
            return Ok(());
        }
        
        if std::path::Path::new(&temp_file).exists() {
            // Process dark mode only if needed (saves 50% operations for light mode)
            if use_temp && self.dark_mode {
                self.process_dark_mode_image(&temp_file, &final_file)?;
                let _ = std::fs::remove_file(&temp_file); // Cleanup temp immediately
            }
            
            self.display_image_in_kitty(&final_file)?;
            self.pdf_image_rendered = true;
        }
        
        Ok(())
    }
    
    fn process_dark_mode_image(&self, input_path: &str, output_path: &str) -> Result<()> {
        
        // Convert PPM to PNG with dark mode processing
        let result = std::process::Command::new("convert")
            .args([
                input_path,
                "-negate", // Invert colors
                "-strip", // Remove metadata
                "-format", "png", // Ensure PNG output
                output_path,
            ])
            .output();
            
        match result {
            Ok(output) => {
                if !output.status.success() {
                    // Fallback: just copy the original file
                    std::fs::copy(input_path, output_path)?;
                }
            }
            Err(_) => {
                // ImageMagick not available, use original image
                std::fs::copy(input_path, output_path)?;
            }
        }
        
        Ok(())
    }
    
    fn display_image_in_kitty(&self, image_path: &str) -> Result<()> {
        // Check if we're actually in kitty terminal
        if !self.is_kitty_terminal() {
            return self.fallback_image_display(image_path);
        }
        
        // Check if the image file exists
        if !std::path::Path::new(image_path).exists() {
            eprintln!("Image file not found: {}", image_path);
            return Ok(());
        }
        
        let split_x = self.terminal_width / 2;
        let base_cols = (split_x - 1).max(10);
        let base_rows = (self.terminal_height - 2).max(10);
        
        // Scale dimensions based on zoom level
        let zoom_factor = self.zoom_level as f32 / 100.0;
        let cols = ((base_cols as f32 * zoom_factor) as u16).max(10);
        let rows = ((base_rows as f32 * zoom_factor) as u16).max(10);
        
        // Center the image in the left panel with minimal margins for maximum size
        let margin_x = 1; // Minimal left margin
        let margin_y = 0; // No top margin for maximum height
        
        // Apply pan offsets to image position (fast movement)
        let display_x = (margin_x as i16 + 1 + self.pan_offset_x).max(1) as u16;
        let display_y = (margin_y as i16 + 1 + self.pan_offset_y).max(1) as u16;
        
        // Ultra-fast image display without clearing (eliminates flash)
        let _ = std::process::Command::new("kitty")
            .args(["+kitten", "icat", 
                   &format!("--place={}x{}@{}x{}", cols - (margin_x * 2), rows - (margin_y * 2), display_x, display_y),
                   "--scale-up",
                   "--transfer-mode=file", // Faster file transfer
                   image_path])
            .status();
        
        Ok(())
    }
    
    fn is_kitty_terminal(&self) -> bool {
        // Check multiple environment variables for Kitty detection
        // KITTY_WINDOW_ID is the most reliable indicator
        if std::env::var("KITTY_WINDOW_ID").is_ok() {
            return true;
        }
        
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        
        term_program == "kitty" || term.contains("kitty") || term == "xterm-kitty"
    }
    
    fn fallback_image_display(&self, image_path: &str) -> Result<()> {
        // Fallback for non-kitty terminals
        let result = std::process::Command::new("open")
            .args(["-a", "Preview", image_path])
            .spawn();
            
        match result {
            Ok(_) => {
                execute!(
                    io::stdout(),
                    cursor::MoveTo(2, 2),
                    SetForegroundColor(Color::Yellow),
                    Print("PDF opened in external viewer"),
                    ResetColor,
                    cursor::MoveTo(2, 3),
                    SetForegroundColor(Color::Yellow),
                    Print("(Terminal doesn't support kitty graphics)"),
                    ResetColor
                )?;
            }
            Err(_) => {
                execute!(
                    io::stdout(),
                    cursor::MoveTo(2, 2),
                    SetForegroundColor(Color::Red),
                    Print("No image display available"),
                    ResetColor,
                    cursor::MoveTo(2, 3),
                    SetForegroundColor(Color::Yellow),
                    Print(format!("File: {}", image_path)),
                    ResetColor
                )?;
            }
        }
        
        Ok(())
    }
    
    fn handle_mouse_click(&mut self, x: u16, y: u16) -> Result<()> {
        // Allow cursor to go anywhere, even beyond viewport
        if y < self.terminal_height - 1 { // Only avoid status line
            if !self.is_selecting {
                // Start new selection
                self.selection_start_x = x;
                self.selection_start_y = y;
                self.selection_end_x = x;
                self.selection_end_y = y;
                self.is_selecting = true;
                self.cursor_x = x;
                self.cursor_y = y;
            } else {
                // Click again - clear selection
                self.is_selecting = false;
                self.cursor_x = x;
                self.cursor_y = y;
            }
        }
        
        Ok(())
    }
    
    fn handle_mouse_drag(&mut self, x: u16, y: u16) -> Result<()> {
        if y < self.terminal_height - 1 && self.is_selecting {
            // Update selection end point during drag
            self.selection_end_x = x;
            self.selection_end_y = y;
            
            // Position cursor at the bottom-right corner of selection
            let max_x = if self.selection_start_x <= self.selection_end_x {
                self.selection_end_x
            } else {
                self.selection_start_x
            };
            let max_y = if self.selection_start_y <= self.selection_end_y {
                self.selection_end_y
            } else {
                self.selection_start_y
            };
            
            self.cursor_x = max_x;
            self.cursor_y = max_y;
        }
        Ok(())
    }
    
    fn handle_key_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        // Handle file picker input separately
        if self.file_picker_open {
            return self.handle_file_picker_input(key, modifiers);
        }
        
        // Debug EVERY key press to see what's happening
        eprintln!("=== KEY PRESS ===");
        eprintln!("Key: {:?}", key);
        eprintln!("Modifiers: {:?}", modifiers);
        eprintln!("AB mode: {}, Active pane: {:?}", self.ab_mode, self.active_pane);
        eprintln!("Page: {}/{}", self.current_page, self.total_pages);
        eprintln!("================");
        
        match key {
            // Pane switching
            KeyCode::Tab => {
                if self.ab_mode {
                    self.active_pane = match self.active_pane {
                        ActivePane::Image => ActivePane::Text,
                        ActivePane::Text => ActivePane::Image,
                    };
                }
            }
            
            // Image pane controls (only when image pane is active)
            KeyCode::Char('=') | KeyCode::Char('+') if self.ab_mode && self.active_pane == ActivePane::Image => {
                // Log zoom attempt
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/chonker95_debug.log")
                    .and_then(|mut file| {
                        use std::io::Write;
                        writeln!(file, "ZOOM IN: {} -> {}", self.zoom_level, 
                            match self.zoom_level { 100 => 150, 150 => 200, 200 => 300, 300 => 400, 400 => 600, _ => self.zoom_level + 100 })
                    });
                self.zoom_in()?;
            }
            KeyCode::Char('-') | KeyCode::Char('_') if self.ab_mode && self.active_pane == ActivePane::Image => {
                // Log zoom attempt
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/chonker95_debug.log")
                    .and_then(|mut file| {
                        use std::io::Write;
                        writeln!(file, "ZOOM OUT: {} -> {}", self.zoom_level,
                            match self.zoom_level { 600 => 400, 400 => 300, 300 => 200, 200 => 150, 150 => 100, _ => if self.zoom_level > 100 { self.zoom_level - 100 } else { 100 } })
                    });
                self.zoom_out()?;
            }
            KeyCode::Char('0') if self.ab_mode && self.active_pane == ActivePane::Image => {
                // Log reset attempt
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/chonker95_debug.log")
                    .and_then(|mut file| {
                        use std::io::Write;
                        writeln!(file, "ZOOM RESET: {} -> 100", self.zoom_level)
                    });
                self.reset_zoom()?;
            }
            
            // Arrow key panning (only when in Image pane)
            KeyCode::Up if self.ab_mode && self.active_pane == ActivePane::Image => {
                self.pan_offset_y -= 5; // Fast movement
                self.pdf_image_rendered = false; // Force re-render with new position
                if self.ab_mode {
                    self.render_pdf_image()?;
                }
            }
            KeyCode::Down if self.ab_mode && self.active_pane == ActivePane::Image => {
                self.pan_offset_y += 5; // Fast movement
                self.pdf_image_rendered = false;
                if self.ab_mode {
                    self.render_pdf_image()?;
                }
            }
            KeyCode::Left if self.ab_mode && self.active_pane == ActivePane::Image => {
                self.pan_offset_x -= 5; // Fast movement
                self.pdf_image_rendered = false;
                if self.ab_mode {
                    self.render_pdf_image()?;
                }
            }
            KeyCode::Right if self.ab_mode && self.active_pane == ActivePane::Image => {
                self.pan_offset_x += 5; // Fast movement
                self.pdf_image_rendered = false;
                if self.ab_mode {
                    self.render_pdf_image()?;
                }
            }
            
            // Global controls (always available)
            KeyCode::Char('a') | KeyCode::Char('A') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.ab_mode = !self.ab_mode;
                self.is_selecting = false; // Clear selection when switching modes
                if self.ab_mode {
                    self.pdf_image_rendered = false; // Force re-render for A-B mode
                    self.active_pane = ActivePane::Image; // Start with image focused in A-B mode
                } else {
                    // Clear screen when exiting A-B mode
                    execute!(io::stdout(), Clear(ClearType::All))?;
                    self.active_pane = ActivePane::Text; // Back to text mode
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.dark_mode = !self.dark_mode;
                if self.ab_mode {
                    self.pdf_image_rendered = false; // Force re-render with new theme
                }
            }
            KeyCode::Char('f') | KeyCode::Char('F') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_file_picker()?;
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(true),
            
            // Global page navigation (works in any mode/pane)
            KeyCode::Char(';') if modifiers.contains(KeyModifiers::CONTROL) => {
                eprintln!("CTRL+; pressed - Previous page attempt, current: {}, total: {}", self.current_page, self.total_pages);
                if self.current_page > 1 {
                    self.current_page -= 1;
                    self.load_page()?;
                    if self.ab_mode {
                        self.pdf_image_rendered = false;
                    }
                    eprintln!("SUCCESS: Changed to page {}", self.current_page);
                }
            }
            KeyCode::Char('\'') if modifiers.contains(KeyModifiers::CONTROL) => {
                eprintln!("CTRL+' pressed - Next page attempt, current: {}, total: {}", self.current_page, self.total_pages);
                if self.current_page < self.total_pages {
                    self.current_page += 1;
                    self.load_page()?;
                    if self.ab_mode {
                        self.pdf_image_rendered = false;
                    }
                    eprintln!("SUCCESS: Changed to page {}", self.current_page);
                }
            }
            
            // Text pane controls (only when text pane is active OR not in A-B mode)
            _ if !self.ab_mode || self.active_pane == ActivePane::Text => {
                self.handle_text_input(key, modifiers)?;
            }
            
            _ => {} // Ignore other keys when in wrong pane
        }
        
        Ok(false)
    }
    
    fn handle_text_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        match key {
            // Cursor movement
            KeyCode::Up => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Arrow: Extend block selection
                    if !self.is_selecting {
                        self.start_block_selection();
                    }
                    if self.selection_end_y > 0 {
                        self.selection_end_y -= 1;
                    }
                    self.update_cursor_to_selection_corner();
                } else if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                }
            }
            KeyCode::Down => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Arrow: Extend block selection
                    if !self.is_selecting {
                        self.start_block_selection();
                    }
                    self.selection_end_y += 1;
                    self.update_cursor_to_selection_corner();
                } else {
                    self.cursor_y += 1; // No bottom limit - can go beyond viewport
                }
            }
            KeyCode::Left => {
                if modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::SHIFT) {
                    // Ctrl+Left: Previous page
                    if self.current_page > 1 {
                        self.current_page -= 1;
                        self.load_page()?;
                    }
                } else if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Arrow: Extend block selection
                    if !self.is_selecting {
                        self.start_block_selection();
                    }
                    if self.selection_end_x > 0 {
                        self.selection_end_x -= 1;
                    }
                    self.update_cursor_to_selection_corner();
                } else if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            KeyCode::Right => {
                if modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::SHIFT) {
                    // Ctrl+Right: Next page
                    if self.current_page < self.total_pages {
                        self.current_page += 1;
                        self.load_page()?;
                    }
                } else if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Arrow: Extend block selection
                    if !self.is_selecting {
                        self.start_block_selection();
                    }
                    self.selection_end_x += 1;
                    self.update_cursor_to_selection_corner();
                } else {
                    self.cursor_x += 1; // No right limit - can go beyond viewport
                }
            }
            KeyCode::Home => {
                self.cursor_x = 0;
            }
            KeyCode::End => {
                // Go to end of current line, not terminal edge
                let lines: Vec<&str> = self.text_buffer.lines().collect();
                if let Some(current_line) = lines.get(self.cursor_y as usize) {
                    self.cursor_x = current_line.len() as u16;
                } else {
                    self.cursor_x = 0; // Empty line
                }
            }
            
            // Text editing
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_char_at_cursor(c)?;
            }
            KeyCode::Backspace => {
                self.delete_char_at_cursor()?;
            }
            KeyCode::Enter => {
                self.insert_char_at_cursor('\n')?;
            }
            
            // Text-specific controls
            KeyCode::Esc => {
                if self.is_selecting {
                    self.is_selecting = false; // Clear selection with Escape
                }
            }
            KeyCode::Char('c') | KeyCode::Char('C') if modifiers.contains(KeyModifiers::CONTROL) => {
                if self.is_selecting {
                    self.copy_selection()?;
                }
            }
            KeyCode::Char('x') | KeyCode::Char('X') if modifiers.contains(KeyModifiers::CONTROL) => {
                if self.is_selecting {
                    self.cut_selection()?;
                }
            }
            KeyCode::Char('v') | KeyCode::Char('V') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.paste_clipboard()?;
            }
            KeyCode::Char('z') | KeyCode::Char('Z') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.undo()?;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.redo()?;
            }
            KeyCode::Char('l') | KeyCode::Char('L') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_layout_mode()?;
            }
            KeyCode::PageUp => {
                if !self.ab_mode {
                    self.scroll_up()?;
                }
            }
            KeyCode::PageDown => {
                if !self.ab_mode {
                    self.scroll_down()?;
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    fn insert_char_at_cursor(&mut self, c: char) -> Result<()> {
        // Save state before modifying
        self.save_to_history();
        let mut lines: Vec<String> = self.text_buffer.lines().map(|s| s.to_string()).collect();
        
        // Ensure we have enough lines for cursor position
        let target_size = (self.cursor_y as usize + 1).min(500);
        while lines.len() < target_size {
            lines.push(String::new());
        }
        
        // Get the current line, extending it if cursor is beyond text
        let current_line = &mut lines[self.cursor_y as usize];
        
        // Extend line with spaces if cursor is beyond current text
        while current_line.len() < self.cursor_x as usize {
            current_line.push(' ');
        }
        
        // Insert character at cursor position
        let cursor_pos = self.cursor_x as usize;
        if c == '\n' {
            // Split line at cursor
            let remaining = current_line.split_off(cursor_pos);
            lines.insert(self.cursor_y as usize + 1, remaining);
            self.cursor_y += 1;
            self.cursor_x = 0;
        } else {
            current_line.insert(cursor_pos, c);
            self.cursor_x += 1;
        }
        
        // Rebuild text buffer
        self.text_buffer = lines.join("\n");
        Ok(())
    }
    
    fn delete_char_at_cursor(&mut self) -> Result<()> {
        // Save state before modifying
        self.save_to_history();
        if self.cursor_x > 0 {
            let lines: Vec<&str> = self.text_buffer.lines().collect();
            let mut new_buffer = String::new();
            
            for (i, line) in lines.iter().enumerate() {
                if i == self.cursor_y as usize {
                    let mut line_chars: Vec<char> = line.chars().collect();
                    let cursor_pos = ((self.cursor_x - 1) as usize).min(line_chars.len());
                    if cursor_pos < line_chars.len() {
                        line_chars.remove(cursor_pos);
                    }
                    new_buffer.push_str(&line_chars.iter().collect::<String>());
                    self.cursor_x -= 1;
                } else {
                    new_buffer.push_str(line);
                }
                
                if i < lines.len() - 1 {
                    new_buffer.push('\n');
                }
            }
            
            self.text_buffer = new_buffer;
        }
        Ok(())
    }
    
    fn start_block_selection(&mut self) {
        self.selection_start_x = self.cursor_x;
        self.selection_start_y = self.cursor_y;
        self.selection_end_x = self.cursor_x;
        self.selection_end_y = self.cursor_y;
        self.is_selecting = true;
    }
    
    fn update_cursor_to_selection_corner(&mut self) {
        if self.is_selecting {
            // Position cursor at the bottom-right corner of selection
            let max_x = if self.selection_start_x <= self.selection_end_x {
                self.selection_end_x
            } else {
                self.selection_start_x
            };
            let max_y = if self.selection_start_y <= self.selection_end_y {
                self.selection_end_y
            } else {
                self.selection_start_y
            };
            
            self.cursor_x = max_x;
            self.cursor_y = max_y;
        }
    }
    
    fn copy_selection(&mut self) -> Result<()> {
        if !self.is_selecting {
            return Ok(());
        }
        
        let lines: Vec<&str> = self.text_buffer.lines().collect();
        
        // Normalize selection bounds
        let (start_y, end_y) = if self.selection_start_y <= self.selection_end_y {
            (self.selection_start_y, self.selection_end_y)
        } else {
            (self.selection_end_y, self.selection_start_y)
        };
        
        let (start_x, end_x) = if self.selection_start_x <= self.selection_end_x {
            (self.selection_start_x, self.selection_end_x)
        } else {
            (self.selection_end_x, self.selection_start_x)
        };
        
        // Extract the selected block of text
        let mut clipboard_lines = Vec::new();
        for y in start_y..=end_y {
            if (y as usize) < lines.len() {
                let line = lines[y as usize];
                let line_chars: Vec<char> = line.chars().collect();
                
                let mut selected_part = String::new();
                for x in start_x..=end_x {
                    if (x as usize) < line_chars.len() {
                        selected_part.push(line_chars[x as usize]);
                    } else {
                        selected_part.push(' '); // Fill spaces for rectangular selection
                    }
                }
                clipboard_lines.push(selected_part.trim_end().to_string());
            } else {
                // Empty line in selection
                clipboard_lines.push("".to_string());
            }
        }
        
        let clipboard_text = clipboard_lines.join("\n");
        
        // Copy to both internal and system clipboard
        self.clipboard = clipboard_lines;
        
        // Copy to system clipboard
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(clipboard_text);
        }
        
        Ok(())
    }
    
    fn cut_selection(&mut self) -> Result<()> {
        if !self.is_selecting {
            return Ok(());
        }
        
        // First copy the selection
        self.copy_selection()?;
        
        // Then delete the selected content
        self.delete_selection()?;
        
        Ok(())
    }
    
    fn delete_selection(&mut self) -> Result<()> {
        if !self.is_selecting {
            return Ok(());
        }
        
        let mut lines: Vec<String> = self.text_buffer.lines().map(|s| s.to_string()).collect();
        
        // Normalize selection bounds
        let (start_y, end_y) = if self.selection_start_y <= self.selection_end_y {
            (self.selection_start_y, self.selection_end_y)
        } else {
            (self.selection_end_y, self.selection_start_y)
        };
        
        let (start_x, end_x) = if self.selection_start_x <= self.selection_end_x {
            (self.selection_start_x, self.selection_end_x)
        } else {
            (self.selection_end_x, self.selection_start_x)
        };
        
        // Delete the selected block
        for y in start_y..=end_y {
            if (y as usize) < lines.len() {
                let line = &mut lines[y as usize];
                let mut line_chars: Vec<char> = line.chars().collect();
                
                // Extend line with spaces if necessary
                while line_chars.len() <= end_x as usize {
                    line_chars.push(' ');
                }
                
                // Remove the selected characters (replace with spaces to maintain layout)
                for x in start_x..=end_x {
                    if (x as usize) < line_chars.len() {
                        line_chars[x as usize] = ' ';
                    }
                }
                
                *line = line_chars.into_iter().collect();
            }
        }
        
        self.text_buffer = lines.join("\n");
        self.is_selecting = false;
        
        Ok(())
    }
    
    fn paste_clipboard(&mut self) -> Result<()> {
        // Save state before modifying
        self.save_to_history();
        // Try system clipboard first, fallback to internal
        let clipboard_content = if let Ok(mut clipboard) = arboard::Clipboard::new() {
            clipboard.get_text().unwrap_or_else(|_| {
                // Fallback to internal clipboard
                self.clipboard.join("\n")
            })
        } else {
            // Fallback to internal clipboard
            self.clipboard.join("\n")
        };
        
        if clipboard_content.is_empty() {
            return Ok(());
        }
        
        let clipboard_lines: Vec<&str> = clipboard_content.lines().collect();
        let mut lines: Vec<String> = self.text_buffer.lines().map(|s| s.to_string()).collect();
        
        // Ensure we have enough lines for the paste operation
        let target_size = (self.cursor_y as usize + clipboard_lines.len() + 1).min(500);
        while lines.len() < target_size {
            lines.push(String::new());
        }
        
        // Paste each line from clipboard starting at cursor position
        for (i, clipboard_line) in clipboard_lines.iter().enumerate() {
            let target_y = self.cursor_y as usize + i;
            if target_y < lines.len() {
                let line = &mut lines[target_y];
                let mut line_chars: Vec<char> = line.chars().collect();
                
                // Extend line with spaces if cursor is beyond current text
                while line_chars.len() < self.cursor_x as usize {
                    line_chars.push(' ');
                }
                
                // Insert clipboard content at cursor position
                for (j, ch) in clipboard_line.chars().enumerate() {
                    let insert_pos = self.cursor_x as usize + j;
                    if insert_pos < line_chars.len() {
                        line_chars[insert_pos] = ch;
                    } else {
                        line_chars.push(ch);
                    }
                }
                
                *line = line_chars.into_iter().collect();
            }
        }
        
        self.text_buffer = lines.join("\n");
        
        Ok(())
    }
    
    fn scan_directory(&mut self) -> Result<()> {
        self.file_list.clear();
        
        // Add parent directory entry if not at root
        if let Some(parent) = self.file_picker_path.parent() {
            if parent != self.file_picker_path {
                let mut parent_path = parent.to_path_buf();
                parent_path.push("..");
                self.file_list.push(parent_path);
            }
        }
        
        // Read directory contents
        if let Ok(entries) = std::fs::read_dir(&self.file_picker_path) {
            let mut dirs = Vec::new();
            let mut files = Vec::new();
            
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().to_lowercase() == "pdf" {
                        files.push(path);
                    }
                }
            }
            
            // Sort directories and files separately
            dirs.sort();
            files.sort();
            
            // Add to file list (directories first, then PDFs)
            self.file_list.extend(dirs);
            self.file_list.extend(files);
        }
        
        // Reset selection to first item
        self.file_picker_selected = 0;
        
        Ok(())
    }
    
    fn render_file_picker(&self) -> Result<()> {
        execute!(io::stdout(), Clear(ClearType::All), cursor::MoveTo(0, 0))?;
        
        // Header
        execute!(
            io::stdout(),
            SetForegroundColor(Color::Cyan),
            Print("📁 File Picker"),
            ResetColor,
            cursor::MoveTo(0, 1),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("Current: {}", self.file_picker_path.display())),
            ResetColor
        )?;
        
        // File list
        let list_start = 3;
        let visible_lines = (self.terminal_height - 5) as usize; // Leave room for header and footer
        let start_index = if self.file_picker_selected >= visible_lines {
            self.file_picker_selected - visible_lines + 1
        } else {
            0
        };
        
        for (i, path) in self.file_list.iter().skip(start_index).take(visible_lines).enumerate() {
            let display_index = start_index + i;
            let y = list_start + i as u16;
            
            execute!(io::stdout(), cursor::MoveTo(0, y))?;
            
            let is_selected = display_index == self.file_picker_selected;
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            
            if is_selected {
                execute!(
                    io::stdout(),
                    SetBackgroundColor(Color::Blue),
                    SetForegroundColor(Color::White)
                )?;
            }
            
            // Icon and name
            if path.is_dir() || filename == ".." {
                execute!(io::stdout(), Print(format!("📁 {}", filename)))?;
            } else {
                execute!(io::stdout(), Print(format!("📄 {}", filename)))?;
            }
            
            if is_selected {
                execute!(io::stdout(), ResetColor)?;
            }
        }
        
        // Footer
        execute!(
            io::stdout(),
            cursor::MoveTo(0, self.terminal_height - 2),
            SetForegroundColor(Color::Yellow),
            Print("↑↓ Navigate • Enter Open • Esc Cancel"),
            ResetColor
        )?;
        
        Ok(())
    }
    
    fn open_file_picker(&mut self) -> Result<()> {
        self.file_picker_open = true;
        self.scan_directory()?;
        Ok(())
    }
    
    fn handle_file_picker_input(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> Result<bool> {
        match key {
            KeyCode::Up => {
                if self.file_picker_selected > 0 {
                    self.file_picker_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.file_picker_selected + 1 < self.file_list.len() {
                    self.file_picker_selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(selected_path) = self.file_list.get(self.file_picker_selected).cloned() {
                    if selected_path.file_name().unwrap_or_default() == ".." {
                        // Navigate to parent directory
                        if let Some(parent) = self.file_picker_path.parent() {
                            self.file_picker_path = parent.to_path_buf();
                            self.scan_directory()?;
                        }
                    } else if selected_path.is_dir() {
                        // Navigate into directory
                        self.file_picker_path = selected_path;
                        self.scan_directory()?;
                    } else if selected_path.extension().map(|ext| ext.to_string_lossy().to_lowercase()) == Some("pdf".to_string()) {
                        // Load the PDF file
                        self.load_new_file(selected_path)?;
                        self.file_picker_open = false;
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.file_picker_open = false;
            }
            _ => {}
        }
        Ok(false)
    }
    
    fn load_new_file(&mut self, path: PathBuf) -> Result<()> {
        self.pdf_path = path;
        self.current_page = 1;
        self.total_pages = 1; // Will be updated in load_page
        self.pdf_image_rendered = false;
        self.is_selecting = false;
        self.cursor_x = 0;
        self.cursor_y = 0;
        // Keep current zoom level when switching files
        self.load_page()?;
        Ok(())
    }
    
    fn zoom_in(&mut self) -> Result<()> {
        let new_zoom = match self.zoom_level {
            100 => 150,
            150 => 200,
            200 => 300,
            300 => 400,
            400 => 600,
            _ => self.zoom_level + 100,
        };
        
        if new_zoom <= 800 { // Max zoom limit
            self.zoom_level = new_zoom;
            self.pan_offset_x = 0; // Reset pan when zoom changes
            self.pan_offset_y = 0;
            self.pdf_image_rendered = false; // Always force re-render at new resolution
            
            // If in A-B mode, immediately re-render the image
            if self.ab_mode {
                self.render_pdf_image()?;
            }
        }
        
        Ok(())
    }
    
    fn zoom_out(&mut self) -> Result<()> {
        let new_zoom = match self.zoom_level {
            600 => 400,
            400 => 300,
            300 => 200,
            200 => 150,
            150 => 100,
            _ => if self.zoom_level > 100 { self.zoom_level - 100 } else { 100 },
        };
        
        if new_zoom >= 50 { // Min zoom limit
            self.zoom_level = new_zoom;
            self.pan_offset_x = 0; // Reset pan when zoom changes
            self.pan_offset_y = 0;
            self.pdf_image_rendered = false; // Always force re-render at new resolution
            
            // If in A-B mode, immediately re-render the image
            if self.ab_mode {
                self.render_pdf_image()?;
            }
        }
        
        Ok(())
    }
    
    fn reset_zoom(&mut self) -> Result<()> {
        if self.zoom_level != 100 {
            self.zoom_level = 100;
            self.pan_offset_x = 0; // Reset pan when zoom resets
            self.pan_offset_y = 0;
            self.pdf_image_rendered = false; // Always force re-render at normal resolution
            
            // If in A-B mode, immediately re-render the image
            if self.ab_mode {
                self.render_pdf_image()?;
            }
        }
        
        Ok(())
    }
    
    
    
    
    fn save_to_history(&mut self) {
        // Truncate history if we're not at the latest position
        if self.history_index >= 0 {
            let keep_until = (self.history_index + 1) as usize;
            self.history.truncate(keep_until);
        }
        
        // Add current state to history
        self.history.push(self.text_buffer.clone());
        
        // Limit history size to prevent memory issues
        const MAX_HISTORY: usize = 50;
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
        
        // Reset to latest position
        self.history_index = -1;
    }
    
    fn undo(&mut self) -> Result<()> {
        if self.history.is_empty() {
            return Ok(());
        }
        
        // If we're at the latest state, save it before going back
        if self.history_index == -1 {
            // We're at current state, move to previous
            self.history_index = (self.history.len() - 1) as isize;
        } else if self.history_index > 0 {
            // Go back one more step
            self.history_index -= 1;
        }
        
        // Restore the historical state
        if self.history_index >= 0 && (self.history_index as usize) < self.history.len() {
            self.text_buffer = self.history[self.history_index as usize].clone();
        }
        
        // Clear any active selection
        self.is_selecting = false;
        
        Ok(())
    }
    
    fn redo(&mut self) -> Result<()> {
        if self.history.is_empty() {
            return Ok(());
        }
        
        // Can only redo if we're in historical position (not at latest)
        if self.history_index >= 0 {
            let next_index = self.history_index + 1;
            if (next_index as usize) < self.history.len() {
                self.history_index = next_index;
                self.text_buffer = self.history[self.history_index as usize].clone();
            } else {
                // Go back to latest (current) state
                self.history_index = -1;
                // Latest state should be the last item in history
                if let Some(latest) = self.history.last() {
                    self.text_buffer = latest.clone();
                }
            }
        }
        
        // Clear any active selection
        self.is_selecting = false;
        
        Ok(())
    }
    
    fn scroll_up(&mut self) -> Result<()> {
        let page_size = (self.terminal_height - 1) as usize;
        self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
        Ok(())
    }
    
    fn scroll_down(&mut self) -> Result<()> {
        let page_size = (self.terminal_height - 1) as usize;
        let total_lines = self.text_buffer.lines().count();
        let max_scroll = total_lines.saturating_sub(page_size);
        
        self.scroll_offset = (self.scroll_offset + page_size).min(max_scroll);
        Ok(())
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Setup terminal - NO alternate screen since it conflicts with kitty graphics
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), event::EnableMouseCapture)?;
    
    let mut editor = WysiwygEditor::new(cli.file, cli.page)?;
    
    loop {
        // Render PDF image if we're in A-B mode and haven't rendered yet
        if editor.ab_mode && !editor.pdf_image_rendered {
            editor.render_pdf_image()?;
        }
        
        editor.render()?;
        
        match event::read()? {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                ..
            }) => {
                editor.handle_mouse_click(x, y)?;
            }
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: x,
                row: y,
                ..
            }) => {
                editor.handle_mouse_drag(x, y)?;
            }
            Event::Key(key_event) => {
                if editor.handle_key_input(key_event.code, key_event.modifiers)? {
                    break; // Quit requested
                }
            }
            _ => {}
        }
    }
    
    // Cleanup terminal
    execute!(
        io::stdout(),
        event::DisableMouseCapture,
        Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    terminal::disable_raw_mode()?;
    
    Ok(())
}