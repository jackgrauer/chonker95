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
        };
        
        editor.load_page()?;
        // Save initial state to history
        editor.save_to_history();
        Ok(editor)
    }
    
    fn load_page(&mut self) -> Result<()> {
        self.elements = self.extract_alto_elements()?;
        self.rebuild_text_buffer();
        Ok(())
    }
    
    fn rebuild_text_buffer(&mut self) {
        if self.elements.is_empty() {
            self.text_buffer = String::new();
            return;
        }
        
        match self.layout_mode {
            LayoutMode::Spatial => self.rebuild_spatial_layout(),
            LayoutMode::Sequential => self.rebuild_sequential_layout(),
        }
    }
    
    fn rebuild_spatial_layout(&mut self) {
        // Calculate page bounds for proper scaling
        let min_hpos = self.elements.iter().map(|e| e.hpos).fold(f32::INFINITY, f32::min);
        let max_hpos = self.elements.iter().map(|e| e.hpos + e._width).fold(f32::NEG_INFINITY, f32::max);
        let min_vpos = self.elements.iter().map(|e| e.vpos).fold(f32::INFINITY, f32::min);
        let max_vpos = self.elements.iter().map(|e| e.vpos + e._height).fold(f32::NEG_INFINITY, f32::max);
        
        let page_width = max_hpos - min_hpos;
        let page_height = max_vpos - min_vpos;
        
        // Create spatial grid for terminal display
        let terminal_width = 120; // Wider for better table handling
        let terminal_height = 60;
        
        // Create 2D grid to place elements
        let mut grid: Vec<Vec<String>> = vec![vec![" ".to_string(); terminal_width]; terminal_height];
        
        // Debug: Log how many elements we have and their types
        let total_elements = self.elements.len();
        let space_elements = self.elements.iter().filter(|e| e.content == " ").count();
        let text_elements = self.elements.iter().filter(|e| e.content != " ").count();
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/chonker95_debug.log")
            .and_then(|mut file| {
                use std::io::Write;
                writeln!(file, "LAYOUT: Total elements: {}, Space elements: {}, Text elements: {}", 
                    total_elements, space_elements, text_elements)
            });

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
            
            // Better coordinate mapping with clustering for nearby elements
            let raw_x = if page_width > 0.0 {
                (element.hpos - min_hpos) / page_width * (terminal_width as f32 - 10.0)
            } else {
                0.0
            };
            
            let raw_y = if page_height > 0.0 {
                (element.vpos - min_vpos) / page_height * (terminal_height as f32 - 5.0)
            } else {
                0.0
            };
            
            // Smart quantization - cluster nearby coordinates
            let x = self.quantize_coordinate(raw_x, 2.0) as usize; // 2-char clustering
            let y = self.quantize_coordinate(raw_y, 1.0) as usize; // 1-line clustering
            
            // Place text in grid, handling overlaps
            let content = if element.content == " " {
                " " // Preserve space elements as-is
            } else {
                element.content.trim()
            };
            
            if y < terminal_height && x < terminal_width && !content.is_empty() {
                // For space elements, just place a single space
                if element.content == " " {
                    if grid[y][x] == " " { // Only place space if position is empty
                        grid[y][x] = " ".to_string();
                    }
                } else {
                    // For text elements, place character by character
                    let chars: Vec<char> = content.chars().collect();
                    for (i, ch) in chars.iter().enumerate() {
                        let pos_x = x + i;
                        if pos_x < terminal_width {
                            // Only overwrite spaces or if this element has higher priority
                            if grid[y][pos_x] == " " || self.has_higher_priority(element, &grid[y][pos_x]) {
                                grid[y][pos_x] = ch.to_string();
                            }
                        }
                    }
                }
            }
        }
        
        // Convert grid back to text buffer
        self.text_buffer = String::new();
        for row in &grid {
            let line: String = row.iter()
                .map(|cell| cell.as_str())
                .collect::<Vec<_>>()
                .join("")
                .trim_end()
                .to_string();
            
            if !line.trim().is_empty() || !self.text_buffer.is_empty() {
                self.text_buffer.push_str(&line);
                self.text_buffer.push('\n');
            }
        }
    }
    
    fn rebuild_sequential_layout(&mut self) {
        // Group elements by line and process sequentially for good text flow
        let min_hpos = self.elements.iter().map(|e| e.hpos).fold(f32::INFINITY, f32::min);
        let max_hpos = self.elements.iter().map(|e| e.hpos + e._width).fold(f32::NEG_INFINITY, f32::max);
        let min_vpos = self.elements.iter().map(|e| e.vpos).fold(f32::INFINITY, f32::min);
        
        let page_width = max_hpos - min_hpos;
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
                            line_output.push_str(&" ".repeat(space_count.min(3)));
                        }
                    }
                }
                
                // Check if line should be centered (like titles, table headers)
                if self.is_line_centered_sequential(&line_elements, page_center, page_width) {
                    let padding = ((terminal_width as i32 - line_output.len() as i32) / 2).max(0) as usize;
                    line_output = format!("{}{}", " ".repeat(padding), line_output);
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
        
        // Rebuild with new layout mode
        self.rebuild_text_buffer();
        
        Ok(())
    }
    
    fn has_higher_priority(&self, element: &AltoElement, existing: &str) -> bool {
        // Prefer actual content over spaces, numbers over letters for tables
        if existing == " " {
            return true;
        }
        
        let content = &element.content;
        // Numbers and money values have higher priority for table alignment
        content.chars().any(|c| c.is_numeric() || c == '$' || c == '%')
    }
    
    fn quantize_coordinate(&self, raw_coord: f32, cluster_size: f32) -> f32 {
        // Cluster nearby coordinates to same terminal position
        (raw_coord / cluster_size).round() * cluster_size
    }
    
    fn is_line_centered(&self, line: &[AltoElement], page_center: f32, page_width: f32) -> bool {
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
        
        let xml_data = String::from_utf8_lossy(&output.stdout);
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
                                let key = String::from_utf8_lossy(attr.key.as_ref());
                                let value = String::from_utf8_lossy(&attr.value);
                                
                                match key.as_ref() {
                                    "CONTENT" => content = value.to_string(),
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
                                let key = String::from_utf8_lossy(attr.key.as_ref());
                                let value = String::from_utf8_lossy(&attr.value);
                                
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
                            " ".to_string(), // Single space character
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
        
        // Only clear screen in text mode
        // In A-B mode with image, we selectively clear only what we need
        if !self.ab_mode {
            execute!(io::stdout(), Clear(ClearType::All), cursor::MoveTo(0, 0))?;
        } else {
            // Don't clear everything - just move cursor
            execute!(io::stdout(), cursor::MoveTo(0, 0))?;
        }
        
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
        
        let status_text = format!("{} - Page {} - {}% - {} (Ctrl+L){}", 
            self.pdf_path.file_stem().unwrap_or_default().to_string_lossy(),
            self.current_page,
            self.zoom_level,
            mode_indicator,
            pane_indicator);
        
        execute!(
            io::stdout(),
            cursor::MoveTo(0, self.terminal_height - 1),
            SetForegroundColor(Color::DarkGrey),
            Print(status_text),
            ResetColor
        )?;
        
        // Position cursor (may be off-screen, that's OK)
        if !self.ab_mode {
            execute!(
                io::stdout(),
                cursor::MoveTo(self.cursor_x, self.cursor_y),
                cursor::Show
            )?;
        }
        
        io::stdout().flush()?;
        Ok(())
    }
    
    fn render_text_only(&self) -> Result<()> {
        // Display the text buffer as continuous text (full width)
        let lines: Vec<&str> = self.text_buffer.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if i < (self.terminal_height - 2) as usize {
                execute!(io::stdout(), cursor::MoveTo(0, i as u16))?;
                self.render_line_with_selection(line, i as u16, 0)?;
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
        
        // Vertical separator with focus indicators
        for y in 0..(self.terminal_height - 1) {
            let separator_color = match self.active_pane {
                ActivePane::Image if y == 0 => Color::Yellow,  // Highlight active pane
                ActivePane::Text if y == 0 => Color::Blue,
                _ => Color::DarkGrey,
            };
            
            let separator_char = if y == 0 {
                match self.active_pane {
                    ActivePane::Image => "┃", // Bold separator for active image pane
                    ActivePane::Text => "│",  // Normal separator
                }
            } else {
                "│"
            };
            
            execute!(
                io::stdout(),
                cursor::MoveTo(split_x, y),
                SetForegroundColor(separator_color),
                Print(separator_char),
                ResetColor
            )?;
        }
        
        // Right side: Text extraction (constrained to right half)
        let lines: Vec<&str> = self.text_buffer.lines().collect();
        let right_panel_width = (self.terminal_width - split_x - 1) as usize;
        
        for (i, line) in lines.iter().enumerate() {
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
            
            // Render up to selection start normally
            for i in 0..norm_start_x as usize {
                if i < line_chars.len() {
                    execute!(io::stdout(), Print(line_chars[i]))?;
                } else {
                    execute!(io::stdout(), Print(' '))?;
                }
            }
            
            // Render selection area with blue background
            execute!(io::stdout(), SetBackgroundColor(Color::Blue), SetForegroundColor(Color::White))?;
            for i in 0..selection_width {
                let char_index = norm_start_x as usize + i;
                if char_index < line_chars.len() {
                    execute!(io::stdout(), Print(line_chars[char_index]))?;
                } else {
                    execute!(io::stdout(), Print(' '))?; // Fill empty spaces with blue
                }
            }
            execute!(io::stdout(), ResetColor)?;
            
            // Render remainder of line normally
            for i in (norm_end_x + 1) as usize..line_chars.len() {
                execute!(io::stdout(), Print(line_chars[i]))?;
            }
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
        
        // Use ghostscript to render PDF page as grayscale JPG
        let temp_file = format!("/tmp/chonker_temp_{}.jpg", self.current_page);
        let output_file = format!("/tmp/chonker_kitty_{}.jpg", self.current_page);
        
        // Clean up any existing files first
        let _ = std::fs::remove_file(&temp_file);
        let _ = std::fs::remove_file(&output_file);
        
        // Log to file instead of terminal
        let _ = std::fs::write("/tmp/chonker95_debug.log", 
            format!("Rendering PDF at {}% zoom ({}dpi) for page {}\n", self.zoom_level, self.zoom_level + 50, self.current_page));
        
        let result = std::process::Command::new("gs")
            .args([
                "-dNOPAUSE", "-dBATCH", "-dSAFER", "-dQUIET",
                "-sDEVICE=jpeggray", // Grayscale JPEG
                &format!("-dFirstPage={}", self.current_page),
                &format!("-dLastPage={}", self.current_page),
                &format!("-r{}", self.zoom_level + 50), // Higher DPI for sharper detail
                "-dJPEGQ=75", // Good quality
                "-dFastWebView", // Optimize for screen display
                "-dNOPLATFONTS", // Skip system font loading
                &format!("-sOutputFile={}", temp_file),
            ])
            .arg(&self.pdf_path)
            .output()?;
            
        if !result.status.success() {
            return Ok(()); // Silently fail if Ghostscript errors
        }
        
        if std::path::Path::new(&temp_file).exists() {
            // Apply dark mode processing if enabled
            if self.dark_mode {
                self.process_dark_mode_image(&temp_file, &output_file)?;
            } else {
                // Just copy the original file
                std::fs::copy(&temp_file, &output_file)?;
            }
            
            self.display_image_in_kitty(&output_file)?;
            self.pdf_image_rendered = true;
            
            // Cleanup temp file
            let _ = std::fs::remove_file(&temp_file);
        }
        
        Ok(())
    }
    
    fn process_dark_mode_image(&self, input_path: &str, output_path: &str) -> Result<()> {
        
        // Use ImageMagick convert to invert colors for pure black background
        let result = std::process::Command::new("convert")
            .args([
                input_path,
                "-negate", // Invert colors (white background becomes black)
                "-level", "0%,90%", // Ensure pure black background, slightly brighten text
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
        
        // Temporarily disable raw mode to display the image
        terminal::disable_raw_mode()?;
        
        // Clear any existing kitty graphics first
        let _ = std::process::Command::new("kitty")
            .args(["+kitten", "icat", "--clear"])
            .status();
        
        // Clear the left panel area
        for y in 0..self.terminal_height {
            execute!(
                io::stdout(),
                cursor::MoveTo(0, y),
                Print(" ".repeat(split_x as usize))
            )?;
        }
        
        // Log image display to file
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/chonker95_debug.log")
            .and_then(|mut file| {
                use std::io::Write;
                writeln!(file, "Displaying image: {} at zoom {}%", image_path, self.zoom_level)
            });
        
        // Display the image using kitty icat with panning and scaling
        let result = std::process::Command::new("kitty")
            .args(["+kitten", "icat", 
                   &format!("--place={}x{}@{}x{}", cols - (margin_x * 2), rows - (margin_y * 2), display_x, display_y),
                   "--scale-up", // Allow upscaling for magnification
                   image_path])
            .status();
            
        // Log result to file
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/chonker95_debug.log")
            .and_then(|mut file| {
                use std::io::Write;
                writeln!(file, "Kitty icat result: {:?}", result)
            });
        
        // Image displayed (debug logging removed)
        
        // Re-enable raw mode
        terminal::enable_raw_mode()?;
        
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
            let (min_x, max_x) = if self.selection_start_x <= self.selection_end_x {
                (self.selection_start_x, self.selection_end_x)
            } else {
                (self.selection_end_x, self.selection_start_x)
            };
            let (min_y, max_y) = if self.selection_start_y <= self.selection_end_y {
                (self.selection_start_y, self.selection_end_y)
            } else {
                (self.selection_end_y, self.selection_start_y)
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
        
        // Log key presses and current state
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/chonker95_debug.log")
            .and_then(|mut file| {
                use std::io::Write;
                writeln!(file, "Key: {:?}, AB mode: {}, Active pane: {:?}", key, self.ab_mode, self.active_pane)
            });
        
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
                    self.current_page += 1;
                    self.load_page()?;
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
            _ => {}
        }
        
        Ok(())
    }
    
    fn insert_char_at_cursor(&mut self, c: char) -> Result<()> {
        // Save state before modifying
        self.save_to_history();
        let mut lines: Vec<String> = self.text_buffer.lines().map(|s| s.to_string()).collect();
        
        // Ensure we have enough lines for cursor position
        while lines.len() <= self.cursor_y as usize {
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
            let (min_x, max_x) = if self.selection_start_x <= self.selection_end_x {
                (self.selection_start_x, self.selection_end_x)
            } else {
                (self.selection_end_x, self.selection_start_x)
            };
            let (min_y, max_y) = if self.selection_start_y <= self.selection_end_y {
                (self.selection_start_y, self.selection_end_y)
            } else {
                (self.selection_end_y, self.selection_start_y)
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
        let mut clipboard_text = String::new();
        
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
        
        clipboard_text = clipboard_lines.join("\n");
        
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
        while lines.len() <= (self.cursor_y as usize + clipboard_lines.len()) {
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
    
    
    fn get_selected_text(&self) -> String {
        let lines: Vec<&str> = self.text_buffer.lines().collect();
        let mut selected_text = String::new();
        
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
        
        // Extract selected block maintaining spatial relationships
        for y in start_y..=end_y {
            if (y as usize) < lines.len() {
                let line = lines[y as usize];
                let line_chars: Vec<char> = line.chars().collect();
                
                let mut row_text = String::new();
                for x in start_x..=end_x {
                    if (x as usize) < line_chars.len() {
                        row_text.push(line_chars[x as usize]);
                    } else {
                        row_text.push(' ');
                    }
                }
                
                if !row_text.trim().is_empty() {
                    selected_text.push_str(&row_text.trim_end());
                    selected_text.push('\n');
                }
            }
        }
        
        selected_text
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