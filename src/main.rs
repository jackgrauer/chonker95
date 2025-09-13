use anyhow::Result;
use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor, SetBackgroundColor},
    terminal::{self, Clear, ClearType},
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use quick_xml::{Reader, events::{Event as XmlEvent, BytesStart}};

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

#[derive(Debug, Clone)]
enum EditorMode {
    Text { 
        scroll_offset: usize 
    },
    SplitScreen { 
        active_pane: ActivePane,
        pan_offset: (i16, i16),
        image_rendered: bool,
    },
    FilePicker {
        path: PathBuf,
        files: Vec<PathBuf>,
        selected: usize,
    },
}

// Only Flow mode - spatial will be applied to selected blocks
#[derive(Debug, Clone, Copy, PartialEq)]
enum LayoutMode {
    Flow, // Natural text flow - always the mode
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

trait LayoutStrategy {
    fn layout(&self, elements: &[AltoElement], context: &LayoutContext) -> String;
}

struct LayoutContext {
    page_bounds: (f32, f32, f32, f32, f32, f32),
    terminal_dims: (usize, usize),
}

#[derive(Debug, Clone, Copy)]
struct PageBounds {
    min_h: f32,
    max_h: f32,
    min_v: f32,
    max_v: f32,
    width: f32,
    height: f32,
    scale_x: f32,
    scale_y: f32,
}

#[derive(Debug, Clone, Copy)]
struct Coordinates {
    pdf: (f32, f32),
    terminal: (u16, u16),
}

impl Coordinates {
    fn from_alto(element: &AltoElement, bounds: &PageBounds, terminal_dims: (usize, usize)) -> Self {
        let x = if bounds.width > 0.0 {
            ((element.hpos - bounds.min_h) * (terminal_dims.0 - 10) as f32 / bounds.width) as u16
        } else { 0 }.min((terminal_dims.0 - 1) as u16);
        
        let y = if bounds.height > 0.0 {
            ((element.vpos - bounds.min_v) * (terminal_dims.1 - 5) as f32 / bounds.height) as u16
        } else { 0 }.min((terminal_dims.1 - 1) as u16);
        
        Self {
            pdf: (element.hpos, element.vpos),
            terminal: (x, y),
        }
    }
}

impl PageBounds {
    fn new(elements: &[AltoElement], terminal_dims: (usize, usize)) -> Self {
        if elements.is_empty() {
            return Self {
                min_h: 0.0, max_h: 0.0, min_v: 0.0, max_v: 0.0,
                width: 0.0, height: 0.0, scale_x: 1.0, scale_y: 1.0,
            };
        }
        
        // Fast single-pass bounds calculation
        let mut min_h = f32::INFINITY;
        let mut max_h = f32::NEG_INFINITY;
        let mut min_v = f32::INFINITY;
        let mut max_v = f32::NEG_INFINITY;
        
        for element in elements {
            min_h = min_h.min(element.hpos);
            max_h = max_h.max(element.hpos + element._width);
            min_v = min_v.min(element.vpos);
            max_v = max_v.max(element.vpos + element._height);
        }
        
        let width = max_h - min_h;
        let height = max_v - min_v;
        
        // Calculate scale factors for terminal mapping
        let scale_x = if width > 0.0 { (terminal_dims.0 - 10) as f32 / width } else { 1.0 };
        let scale_y = if height > 0.0 { (terminal_dims.1 - 5) as f32 / height } else { 1.0 };
        
        Self {
            min_h, max_h, min_v, max_v, width, height, scale_x, scale_y
        }
    }
}

struct FlowLayout;
struct SpatialLayout;

#[derive(Clone, Copy)]
struct Selection {
    start: (u16, u16),
    end: (u16, u16),
    active: bool,
}

impl Selection {
    fn new() -> Self {
        Self {
            start: (0, 0),
            end: (0, 0),
            active: false,
        }
    }
    
    fn normalized(&self) -> ((u16, u16), (u16, u16)) {
        let (sx, sy) = self.start;
        let (ex, ey) = self.end;
        (
            (sx.min(ex), sy.min(ey)),
            (sx.max(ex), sy.max(ey))
        )
    }
    
    fn contains(&self, x: u16, y: u16) -> bool {
        if !self.active {
            return false;
        }
        let ((min_x, min_y), (max_x, max_y)) = self.normalized();
        x >= min_x && x <= max_x && y >= min_y && y <= max_y
    }
    
    fn extract_text(&self, buffer: &str) -> String {
        if !self.active {
            return String::new();
        }
        
        let ((min_x, min_y), (max_x, max_y)) = self.normalized();
        buffer.lines()
            .skip(min_y as usize)
            .take((max_y - min_y + 1) as usize)
            .map(|line| {
                let chars: Vec<char> = line.chars().collect();
                let start_idx = (min_x as usize).min(chars.len());
                let end_idx = ((max_x + 1) as usize).min(chars.len());
                if end_idx > start_idx {
                    chars[start_idx..end_idx].iter().collect()
                } else {
                    String::new()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    
    fn start_at(&mut self, x: u16, y: u16) {
        self.start = (x, y);
        self.end = (x, y);
        self.active = true;
    }
    
    fn extend_to(&mut self, x: u16, y: u16) {
        if self.active {
            self.end = (x, y);
        }
    }
    
    fn clear(&mut self) {
        self.active = false;
    }
}

struct PdfRenderer {
    pdf_path: PathBuf,
    current_page: u32,
    zoom_level: u16,
    dark_mode: bool,
}

impl PdfRenderer {
    fn new(pdf_path: PathBuf, page: u32, zoom: u16, dark: bool) -> Self {
        Self {
            pdf_path,
            current_page: page,
            zoom_level: zoom,
            dark_mode: dark,
        }
    }
    
    fn render_to_kitty(&self, pan_offset: (i16, i16), terminal_dims: (u16, u16)) -> Result<()> {
        let temp_file = self.generate_temp_filename(".ppm");
        let final_file = if self.dark_mode {
            self.generate_temp_filename(".png")
        } else {
            temp_file.clone()
        };
        
        // Clean up existing files
        let _ = std::fs::remove_file(&final_file);
        if self.dark_mode {
            let _ = std::fs::remove_file(&temp_file);
        }
        
        // Render PDF to temporary file
        self.render_pdf_to_file(&temp_file)?;
        
        // Process for dark mode if needed
        if self.dark_mode {
            self.apply_dark_mode(&temp_file, &final_file)?;
            let _ = std::fs::remove_file(&temp_file);
        }
        
        // Display in kitty with positioning
        self.display_in_kitty(&final_file, pan_offset, terminal_dims)?;
        
        Ok(())
    }
    
    fn generate_temp_filename(&self, extension: &str) -> String {
        format!("/tmp/chonker_p{}_{}{}", self.current_page, self.zoom_level, extension)
    }
    
    fn render_pdf_to_file(&self, output_path: &str) -> Result<()> {
        let target_dpi = (self.zoom_level as f32 * 0.7) as u16;
        let result = std::process::Command::new("gs")
            .args([
                "-dNOPAUSE", "-dBATCH", "-dQUIET", "-dNOSAFER",
                "-sDEVICE=ppmraw",
                &format!("-dFirstPage={}", self.current_page),
                &format!("-dLastPage={}", self.current_page),
                &format!("-r{}", target_dpi),
                &format!("-sOutputFile={}", output_path),
            ])
            .arg(&self.pdf_path)
            .output()?;
            
        if !result.status.success() {
            return Err(anyhow::anyhow!("Ghostscript failed"));
        }
        
        Ok(())
    }
    
    fn apply_dark_mode(&self, input_path: &str, output_path: &str) -> Result<()> {
        let result = std::process::Command::new("convert")
            .args([
                input_path,
                "-negate",
                "-strip", 
                "-format", "png",
                output_path,
            ])
            .output();
            
        match result {
            Ok(output) if output.status.success() => Ok(()),
            _ => {
                // Fallback: copy original if ImageMagick fails
                std::fs::copy(input_path, output_path)?;
                Ok(())
            }
        }
    }
    
    fn display_in_kitty(&self, image_path: &str, pan_offset: (i16, i16), terminal_dims: (u16, u16)) -> Result<()> {
        if !Self::is_kitty_terminal() {
            return self.fallback_display(image_path);
        }
        
        if !std::path::Path::new(image_path).exists() {
            return Err(anyhow::anyhow!("Image file not found: {}", image_path));
        }
        
        let split_x = terminal_dims.0 / 2;
        let base_cols = (split_x - 1).max(10);
        let base_rows = (terminal_dims.1 - 2).max(10);
        
        let zoom_factor = self.zoom_level as f32 / 100.0;
        let cols = ((base_cols as f32 * zoom_factor) as u16).max(10);
        let rows = ((base_rows as f32 * zoom_factor) as u16).max(10);
        
        let display_x = (1 + pan_offset.0).max(1) as u16;
        let display_y = (1 + pan_offset.1).max(1) as u16;
        
        let _ = std::process::Command::new("kitty")
            .args(["+kitten", "icat", 
                   &format!("--place={}x{}@{}x{}", cols - 2, rows - 2, display_x, display_y),
                   "--scale-up",
                   "--transfer-mode=file",
                   image_path])
            .status();
        
        Ok(())
    }
    
    fn is_kitty_terminal() -> bool {
        std::env::var("KITTY_WINDOW_ID").is_ok() ||
        std::env::var("TERM_PROGRAM").unwrap_or_default() == "kitty" ||
        std::env::var("TERM").unwrap_or_default().contains("kitty")
    }
    
    fn fallback_display(&self, image_path: &str) -> Result<()> {
        std::process::Command::new("open")
            .args(["-a", "Preview", image_path])
            .spawn()?;
        Ok(())
    }
}

struct TerminalRenderer {
    _buffer: Vec<String>, // For future batching if needed
}

impl TerminalRenderer {
    fn new() -> Self {
        Self { _buffer: Vec::new() }
    }
    
    fn clear_screen(self) -> Self {
        execute!(io::stdout(), Clear(ClearType::All)).ok();
        self
    }
    
    fn clear_area(self, x: u16, y: u16, width: usize, height: usize) -> Self {
        for row in 0..height {
            execute!(
                io::stdout(),
                cursor::MoveTo(x, y + row as u16),
                Print(" ".repeat(width))
            ).ok();
        }
        self
    }
    
    fn draw_text(self, x: u16, y: u16, text: &str, color: Color) -> Self {
        execute!(
            io::stdout(),
            cursor::MoveTo(x, y),
            SetForegroundColor(color),
            Print(text),
            ResetColor
        ).ok();
        self
    }
    
    fn draw_text_default(self, x: u16, y: u16, text: &str) -> Self {
        execute!(
            io::stdout(),
            cursor::MoveTo(x, y),
            Print(text)
        ).ok();
        self
    }
    
    fn draw_status_line(self, y: u16, text: &str) -> Self {
        execute!(
            io::stdout(),
            cursor::MoveTo(0, y),
            SetForegroundColor(Color::DarkGrey),
            Print(text),
            ResetColor
        ).ok();
        self
    }
    
    fn move_cursor(self, x: u16, y: u16) -> Self {
        execute!(io::stdout(), cursor::MoveTo(x, y)).ok();
        self
    }
    
    fn show_cursor(self) -> Self {
        execute!(io::stdout(), cursor::Show).ok();
        self
    }
    
    fn hide_cursor(self) -> Self {
        execute!(io::stdout(), cursor::Hide).ok();
        self
    }
    
    fn draw_line_with_selection(self, x: u16, y: u16, line: &str, selection: &Selection, col_offset: u16) -> Self {
        if !selection.active {
            return self.draw_text_default(x, y, line);
        }
        
        let ((min_x, min_y), (max_x, max_y)) = selection.normalized();
        
        if y >= min_y && y <= max_y {
            let adj_min_x = min_x.saturating_sub(col_offset);
            let adj_max_x = max_x.saturating_sub(col_offset);
            
            let line_chars: Vec<char> = line.chars().collect();
            
            // Build the line parts
            let pre_selection = if adj_min_x > 0 {
                line_chars.iter().take(adj_min_x as usize).collect::<String>()
            } else {
                String::new()
            };
            
            let selection_content = line_chars.iter()
                .skip(adj_min_x as usize)
                .take((adj_max_x - adj_min_x + 1) as usize)
                .collect::<String>();
                
            let post_selection = line_chars.iter()
                .skip((adj_max_x + 1) as usize)
                .collect::<String>();
            
            // Render in parts
            execute!(io::stdout(), cursor::MoveTo(x, y), Print(&pre_selection)).ok();
            execute!(io::stdout(), SetBackgroundColor(Color::Blue), SetForegroundColor(Color::White), Print(&selection_content), ResetColor).ok();
            execute!(io::stdout(), Print(&post_selection)).ok();
        } else {
            execute!(io::stdout(), cursor::MoveTo(x, y), Print(line)).ok();
        }
        
        self
    }
    
    fn render(self) {
        io::stdout().flush().ok();
    }
}

impl LayoutStrategy for FlowLayout {
    fn layout(&self, elements: &[AltoElement], context: &LayoutContext) -> String {
        let bounds = PageBounds::new(elements, context.terminal_dims);
        let page_center = bounds.min_h + (bounds.width / 2.0);
        
        // Group elements by line with better clustering
        let mut lines_map = std::collections::HashMap::new();
        for element in elements {
            let line_y = ((element.vpos - bounds.min_v) / 8.0) as usize; // Tighter line grouping
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
                if Self::is_line_centered(&line_elements, page_center, bounds.width) {
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
        
        output_lines.join("\n")
    }
}

impl FlowLayout {
    fn is_line_centered(line: &[AltoElement], page_center: f32, page_width: f32) -> bool {
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
}

impl LayoutStrategy for SpatialLayout {
    fn layout(&self, elements: &[AltoElement], context: &LayoutContext) -> String {
        let bounds = PageBounds::new(elements, context.terminal_dims);
        
        // Dynamic grid sizing based on actual content bounds
        let terminal_width = ((bounds.width / 6.0) as usize + 20).min(200).max(80);
        let terminal_height = ((bounds.height / 12.0) as usize + 10).min(100).max(30);
        
        let mut grid = vec![' '; terminal_width * terminal_height];
        
        // Map each element to terminal grid using coordinate struct
        for element in elements {
            let content = if element.content == " " {
                " "
            } else {
                element.content.trim()
            };
            
            if content.is_empty() && element.content != " " {
                continue;
            }
            
            // Use clean coordinate conversion
            let coords = Coordinates::from_alto(element, &bounds, (terminal_width, terminal_height));
            let x = coords.terminal.0 as usize;
            let y = coords.terminal.1 as usize;
            
            if y < terminal_height && x < terminal_width && !content.is_empty() {
                let grid_idx = y * terminal_width + x;
                if grid_idx < grid.len() {
                    if element.content == " " {
                        if grid[grid_idx] == ' ' {
                            grid[grid_idx] = ' ';
                        }
                    } else {
                        // For text elements, place character by character
                        for (i, ch) in content.char_indices() {
                            let pos_x = x + i;
                            if pos_x < terminal_width {
                                let pos_idx = y * terminal_width + pos_x;
                                if pos_idx < grid.len() {
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
            
        buffer_lines.join("\n")
    }
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
    
    fn from_attrs(attrs: HashMap<String, String>, element_type: ElementType) -> Self {
        let content = match element_type {
            ElementType::Text => attrs.get("CONTENT").cloned().unwrap_or_default(),
            ElementType::Space => " ".to_string(),
        };
        
        let hpos = attrs.get("HPOS").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let vpos = attrs.get("VPOS").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let width = attrs.get("WIDTH").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let height = attrs.get("HEIGHT").and_then(|v| v.parse().ok()).unwrap_or(10.0);
        
        Self::new(
            format!("elem_{}", hpos as u32), // Simple ID based on position
            content,
            hpos,
            vpos,
            width,
            height,
        )
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
    // Editor mode (replaces ab_mode, file_picker_open, etc.)
    mode: EditorMode,
    // Dark mode for PDF display
    dark_mode: bool,
    // Block text selection
    selection: Selection,
    // Clipboard for copy/paste
    clipboard: Vec<String>,
    // Zoom controls
    zoom_level: u16, // Resolution multiplier (100, 150, 200, 300, etc.)
    // Undo/redo system
    history: Vec<String>,
    history_index: isize, // -1 means at latest, 0+ means historical position
    // Layout mode
    layout_mode: LayoutMode,
    // Performance: dirty tracking
    text_buffer_dirty: bool,
    last_layout_mode: LayoutMode,
    // Total pages in PDF
    total_pages: u32,
}

impl WysiwygEditor {
    fn new(pdf_path: PathBuf, page: u32) -> Result<Self> {
        let (width, height) = terminal::size()?;
        let mut editor = Self {
            elements: Vec::new(),
            pdf_path,
            current_page: page,
            terminal_width: width,
            terminal_height: height,
            text_buffer: String::new(),
            cursor_x: 0,
            cursor_y: 0,
            mode: EditorMode::Text { scroll_offset: 0 },
            dark_mode: true, // Default to dark mode
            selection: Selection::new(),
            clipboard: Vec::new(),
            zoom_level: 100, // Start at 100 DPI
            history: Vec::new(),
            history_index: -1,
            layout_mode: LayoutMode::Flow, // Start with natural text flow
            text_buffer_dirty: true, // Needs initial build
            last_layout_mode: LayoutMode::Flow,
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
        if let EditorMode::Text { scroll_offset } = &mut self.mode {
            *scroll_offset = 0; // Reset scroll when loading new page
        }
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
        
        let context = LayoutContext {
            page_bounds: self.calculate_page_bounds(),
            terminal_dims: (self.terminal_width as usize, self.terminal_height as usize),
        };
        
        // Always use Flow layout for the whole document
        let strategy = FlowLayout;
        self.text_buffer = strategy.layout(&self.elements, &context);
        
        // Mark as clean and update last mode
        self.text_buffer_dirty = false;
        self.last_layout_mode = self.layout_mode;
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
    
    
    fn handle_image_pane_input(&mut self, key: KeyCode) -> Result<bool> {
        let (active_pane, pan_offset, image_rendered) = match &mut self.mode {
            EditorMode::SplitScreen { active_pane, pan_offset, image_rendered } => {
                (active_pane, pan_offset, image_rendered)
            }
            _ => return Ok(false),
        };
        
        if *active_pane != ActivePane::Image {
            return Ok(false);
        }
        
        match key {
            KeyCode::Char('=') | KeyCode::Char('+') => {
                self.zoom_in()?;
                Ok(true)
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.zoom_out()?;
                Ok(true)
            }
            KeyCode::Char('0') => {
                self.reset_zoom()?;
                Ok(true)
            }
            KeyCode::Up => {
                pan_offset.1 -= 5;
                *image_rendered = false;
                self.render_pdf_image()?;
                Ok(true)
            }
            KeyCode::Down => {
                pan_offset.1 += 5;
                *image_rendered = false;
                self.render_pdf_image()?;
                Ok(true)
            }
            KeyCode::Left => {
                pan_offset.0 -= 5;
                *image_rendered = false;
                self.render_pdf_image()?;
                Ok(true)
            }
            KeyCode::Right => {
                pan_offset.0 += 5;
                *image_rendered = false;
                self.render_pdf_image()?;
                Ok(true)
            }
            _ => Ok(false),
        }
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
        let mut reader = Reader::from_str(xml);
        let mut buf = Vec::new();
        let mut elements = Vec::new();
        
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(XmlEvent::Start(e)) | Ok(XmlEvent::Empty(e)) => {
                    if let Some(element) = self.parse_element(&e) {
                        elements.push(element);
                    }
                }
                Ok(XmlEvent::Eof) => break,
                _ => {}
            }
            buf.clear();
        }
        
        elements
    }
    
    fn parse_element(&self, e: &BytesStart) -> Option<AltoElement> {
        let attrs: HashMap<String, String> = e.attributes()
            .filter_map(Result::ok)
            .map(|a| {
                (
                    String::from_utf8_lossy(a.key.as_ref()).to_string(),
                    String::from_utf8_lossy(&a.value).to_string()
                )
            })
            .collect();
        
        match e.name().as_ref() {
            b"String" => {
                let content = attrs.get("CONTENT").cloned().unwrap_or_default();
                if !content.is_empty() {
                    Some(AltoElement::from_attrs(attrs, ElementType::Text))
                } else {
                    None
                }
            }
            b"SP" => Some(AltoElement::from_attrs(attrs, ElementType::Space)),
            _ => None
        }
    }
    
    fn render(&self) -> Result<()> {
        match &self.mode {
            EditorMode::Text { scroll_offset } => {
                TerminalRenderer::new().clear_screen().render();
                self.render_text_only(*scroll_offset)?;
            }
            EditorMode::SplitScreen { .. } => {
                // In split screen mode, don't clear - just update what's needed
                self.render_split_screen()?;
            }
            EditorMode::FilePicker { .. } => {
                self.render_file_picker()?;
                return Ok(()); // Skip status line for file picker
            }
        }
        
        // Status line - always FLOW mode now
        let mode_indicator = "FLOW";
        
        let (pane_indicator, scroll_info) = match &self.mode {
            EditorMode::Text { scroll_offset } => {
                let total_lines = self.text_buffer.lines().count();
                let visible_lines = (self.terminal_height - 1) as usize;
                let scroll_info = if total_lines > visible_lines {
                    format!(" - Line {}/{}", scroll_offset + 1, total_lines)
                } else {
                    String::new()
                };
                ("", scroll_info)
            }
            EditorMode::SplitScreen { active_pane, .. } => {
                let pane_info = match active_pane {
                    ActivePane::Image => " - [IMAGE]",
                    ActivePane::Text => " - [TEXT]",
                };
                (pane_info, String::new())
            }
            EditorMode::FilePicker { .. } => ("", String::new()),
        };
        
        let status_text = format!("{} - Page {}/{} - {}% - {}{}{}", 
            self.pdf_path.file_stem().unwrap_or_default().to_string_lossy(),
            self.current_page,
            self.total_pages,
            self.zoom_level,
            mode_indicator,
            pane_indicator,
            scroll_info);
        
        // Use builder pattern for cleaner rendering
        let mut renderer = TerminalRenderer::new()
            .draw_status_line(self.terminal_height - 1, &status_text);
        
        // Position cursor based on mode
        match &self.mode {
            EditorMode::Text { .. } => {
                renderer = renderer
                    .move_cursor(self.cursor_x, self.cursor_y)
                    .show_cursor();
            }
            EditorMode::SplitScreen { .. } => {
                renderer = renderer.hide_cursor();
            }
            EditorMode::FilePicker { .. } => {
                // File picker handles its own cursor
            }
        }
        
        renderer.render();
        Ok(())
    }
    
    fn render_text_only(&self, scroll_offset: usize) -> Result<()> {
        // Calculate how many lines we can show (leave room for status line)
        let available_lines = (self.terminal_height - 1) as usize;
        let total_lines: Vec<&str> = self.text_buffer.lines().collect();
        
        // Clear text area using builder pattern
        let mut renderer = TerminalRenderer::new()
            .clear_area(0, 0, self.terminal_width as usize, available_lines);
        
        // Render visible lines starting from scroll_offset
        for (screen_line, text_line_index) in (scroll_offset..).enumerate() {
            if screen_line >= available_lines {
                break;
            }
            
            if let Some(line) = total_lines.get(text_line_index) {
                // For now, use direct rendering for selection - will improve this later
                execute!(io::stdout(), cursor::MoveTo(0, screen_line as u16))?;
                self.render_line_with_selection(line, screen_line as u16, 0)?;
            }
        }
        
        renderer.render();
        Ok(())
    }
    
    fn render_split_screen(&self) -> Result<()> {
        let split_x = self.terminal_width / 2;
        
        // Left side: PDF image area 
        let image_rendered = matches!(self.mode, EditorMode::SplitScreen { image_rendered: true, .. });
        if !image_rendered {
            TerminalRenderer::new()
                .draw_text(2, 2, "Loading PDF image...", Color::Cyan)
                .draw_text(2, 4, &format!("Page: {}", self.current_page), Color::Yellow)
                .draw_text(2, 5, &format!("File: {}", self.pdf_path.display()), Color::Yellow)
                .render();
        } else {
            // Don't overwrite the image area - it should be displayed
            // The image is rendered at position 0,0 and takes up the left panel
            // We don't print anything over it
        }
        
        // Clean vertical separator with subtle focus indication
        let separator_color = match &self.mode {
            EditorMode::SplitScreen { active_pane: ActivePane::Image, .. } => Color::Blue,
            _ => Color::DarkGrey,
        };
        
        let mut renderer = TerminalRenderer::new();
        for y in 0..(self.terminal_height - 1) {
            renderer = renderer.draw_text(split_x, y, "│", separator_color);
        }
        renderer.render();
        
        // Right side: Text extraction with clean black background
        let right_panel_width = (self.terminal_width - split_x - 1) as usize;
        
        // Clear right panel using builder pattern
        TerminalRenderer::new()
            .clear_area(split_x + 1, 0, right_panel_width, (self.terminal_height - 1) as usize)
            .render();
        
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
        if !self.selection.active {
            // No selection, just print normally
            execute!(io::stdout(), Print(line))?;
            return Ok(());
        }
        
        // Check if this row is within selection using simplified logic
        let ((min_x, min_y), (max_x, max_y)) = self.selection.normalized();
        
        if row >= min_y && row <= max_y {
            let adj_min_x = min_x.saturating_sub(col_offset);
            let adj_max_x = max_x.saturating_sub(col_offset);
            
            let line_chars: Vec<char> = line.chars().collect();
            
            // Build the line parts
            let pre_selection = if adj_min_x > 0 {
                line_chars.iter().take(adj_min_x as usize).collect::<String>()
            } else {
                String::new()
            };
            
            let selection_content = line_chars.iter()
                .skip(adj_min_x as usize)
                .take((adj_max_x - adj_min_x + 1) as usize)
                .collect::<String>();
                
            let post_selection = line_chars.iter()
                .skip((adj_max_x + 1) as usize)
                .collect::<String>();
            
            // Render in one go to reduce flicker
            execute!(io::stdout(), Print(&pre_selection))?;
            execute!(io::stdout(), SetBackgroundColor(Color::Blue), SetForegroundColor(Color::White), Print(&selection_content), ResetColor)?;
            execute!(io::stdout(), Print(&post_selection))?;
        } else {
            // Row is not in selection - normal rendering
            execute!(io::stdout(), Print(line))?;
        }
        
        Ok(())
    }
    
    fn render_pdf_image(&mut self) -> Result<()> {
        let (pan_offset, image_rendered) = match &mut self.mode {
            EditorMode::SplitScreen { pan_offset, image_rendered, .. } => {
                (pan_offset, image_rendered)
            }
            _ => return Ok(()),
        };
        
        if *image_rendered {
            return Ok(());
        }
        
        let renderer = PdfRenderer::new(
            self.pdf_path.clone(),
            self.current_page,
            self.zoom_level,
            self.dark_mode,
        );
        
        renderer.render_to_kitty(
            *pan_offset,
            (self.terminal_width, self.terminal_height),
        )?;
        
        *image_rendered = true;
        Ok(())
    }
    
    
    fn handle_mouse_click(&mut self, x: u16, y: u16) -> Result<()> {
        // Allow cursor to go anywhere, even beyond viewport
        if y < self.terminal_height - 1 { // Only avoid status line
            if !self.selection.active {
                // Start new selection
                self.selection.start_at(x, y);
                self.cursor_x = x;
                self.cursor_y = y;
            } else {
                // Click again - clear selection
                self.selection.clear();
                self.cursor_x = x;
                self.cursor_y = y;
            }
        }
        
        Ok(())
    }
    
    fn handle_mouse_drag(&mut self, x: u16, y: u16) -> Result<()> {
        if y < self.terminal_height - 1 && self.selection.active {
            // Update selection end point during drag
            self.selection.extend_to(x, y);
            
            // Position cursor at the bottom-right corner of selection
            let ((_, _), (max_x, max_y)) = self.selection.normalized();
            self.cursor_x = max_x;
            self.cursor_y = max_y;
        }
        Ok(())
    }
    
    fn handle_key_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        // Handle file picker input separately
        if matches!(self.mode, EditorMode::FilePicker { .. }) {
            return self.handle_file_picker_input(key, modifiers);
        }
        
        // Debug key presses
        eprintln!("Key: {:?}, Modifiers: {:?}, Page: {}/{}", key, modifiers, self.current_page, self.total_pages);
        
        // Handle commands with clean pattern matching
        match (key, modifiers) {
            // Global commands - quit with q, Q, or Ctrl+Q
            (KeyCode::Char('q'), KeyModifiers::NONE) | 
            (KeyCode::Char('Q'), KeyModifiers::NONE) |
            (KeyCode::Char('q'), KeyModifiers::CONTROL) |
            (KeyCode::Char('Q'), KeyModifiers::CONTROL) => {
                return Ok(true);
            }
            
            // File operations
            (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                let current_path = self.pdf_path.parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .to_path_buf();
                self.mode = EditorMode::FilePicker {
                    path: current_path,
                    files: Vec::new(),
                    selected: 0,
                };
                self.scan_directory()?;
                return Ok(false);
            }
            
            // Mode operations
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.selection.clear();
                match &self.mode {
                    EditorMode::Text { .. } => {
                        self.mode = EditorMode::SplitScreen {
                            active_pane: ActivePane::Image,
                            pan_offset: (0, 0),
                            image_rendered: false,
                        };
                    }
                    EditorMode::SplitScreen { .. } => {
                        execute!(io::stdout(), Clear(ClearType::All))?;
                        self.mode = EditorMode::Text { scroll_offset: 0 };
                    }
                    EditorMode::FilePicker { .. } => {
                        // Switch from file picker to text mode
                        self.mode = EditorMode::Text { scroll_offset: 0 };
                    }
                }
                return Ok(false);
            }
            
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.dark_mode = !self.dark_mode;
                if let EditorMode::SplitScreen { image_rendered, .. } = &mut self.mode {
                    *image_rendered = false;
                }
                return Ok(false);
            }
            
            // Page navigation
            (KeyCode::Char(';'), KeyModifiers::CONTROL) => {
                eprintln!("CTRL+; pressed - Previous page attempt, current: {}, total: {}", self.current_page, self.total_pages);
                if self.current_page > 1 {
                    self.current_page -= 1;
                    self.load_page()?;
                    if let EditorMode::SplitScreen { image_rendered, .. } = &mut self.mode {
                        *image_rendered = false;
                    }
                    eprintln!("SUCCESS: Changed to page {}", self.current_page);
                }
                return Ok(false);
            }
            
            (KeyCode::Char('\''), KeyModifiers::CONTROL) => {
                eprintln!("CTRL+' pressed - Next page attempt, current: {}, total: {}", self.current_page, self.total_pages);
                if self.current_page < self.total_pages {
                    self.current_page += 1;
                    self.load_page()?;
                    if let EditorMode::SplitScreen { image_rendered, .. } = &mut self.mode {
                        *image_rendered = false;
                    }
                    eprintln!("SUCCESS: Changed to page {}", self.current_page);
                }
                return Ok(false);
            }
            
            // Selection and editing - removed spatial selection stub
            
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.selection.active {
                    self.copy_selection()?;
                }
                return Ok(false);
            }
            
            (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                if self.selection.active {
                    self.cut_selection()?;
                }
                return Ok(false);
            }
            
            (KeyCode::Char('v'), KeyModifiers::CONTROL) => {
                self.paste_clipboard()?;
                return Ok(false);
            }
            
            (KeyCode::Char('z'), KeyModifiers::CONTROL) => {
                self.undo()?;
                return Ok(false);
            }
            
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                self.redo()?;
                return Ok(false);
            }
            
            // Pane switching
            (KeyCode::Tab, KeyModifiers::NONE) => {
                if let EditorMode::SplitScreen { active_pane, .. } = &mut self.mode {
                    *active_pane = match active_pane {
                        ActivePane::Image => ActivePane::Text,
                        ActivePane::Text => ActivePane::Image,
                    };
                }
                return Ok(false);
            }
            
            _ => {} // Fall through to other handlers
        }
        
        // Handle image pane controls with dedicated method
        if self.handle_image_pane_input(key)? {
            return Ok(false);
        }
        
        // Handle text input based on mode
        match &self.mode {
            EditorMode::Text { .. } => {
                self.handle_text_input(key, modifiers)?;
            }
            EditorMode::SplitScreen { active_pane: ActivePane::Text, .. } => {
                self.handle_text_input(key, modifiers)?;
            }
            _ => {} // No text input in other modes
        }
        
        Ok(false)
    }
    
    fn handle_text_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        match key {
            // Cursor movement
            KeyCode::Up => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Arrow: Extend block selection
                    if !self.selection.active {
                        self.start_block_selection();
                    }
                    let (_, current_y) = self.selection.end;
                    if current_y > 0 {
                        self.selection.extend_to(self.selection.end.0, current_y - 1);
                    }
                    self.update_cursor_to_selection_corner();
                } else if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                }
            }
            KeyCode::Down => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Arrow: Extend block selection
                    if !self.selection.active {
                        self.start_block_selection();
                    }
                    let (current_x, current_y) = self.selection.end;
                    self.selection.extend_to(current_x, current_y + 1);
                    self.update_cursor_to_selection_corner();
                } else {
                    self.cursor_y += 1; // No bottom limit - can go beyond viewport
                }
            }
            KeyCode::Left => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Arrow: Extend block selection
                    if !self.selection.active {
                        self.start_block_selection();
                    }
                    let (current_x, current_y) = self.selection.end;
                    if current_x > 0 {
                        self.selection.extend_to(current_x - 1, current_y);
                    }
                    self.update_cursor_to_selection_corner();
                } else if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            KeyCode::Right => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Arrow: Extend block selection
                    if !self.selection.active {
                        self.start_block_selection();
                    }
                    let (current_x, current_y) = self.selection.end;
                    self.selection.extend_to(current_x + 1, current_y);
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
                if self.selection.active {
                    self.selection.clear(); // Clear selection with Escape
                }
            }
            KeyCode::PageUp => {
                if let EditorMode::Text { scroll_offset } = &mut self.mode {
                    let page_size = (self.terminal_height - 1) as usize;
                    *scroll_offset = scroll_offset.saturating_sub(page_size);
                }
            }
            KeyCode::PageDown => {
                if let EditorMode::Text { scroll_offset } = &mut self.mode {
                    let page_size = (self.terminal_height - 1) as usize;
                    let total_lines = self.text_buffer.lines().count();
                    let max_scroll = total_lines.saturating_sub(page_size);
                    *scroll_offset = (*scroll_offset + page_size).min(max_scroll);
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    fn insert_char_at_cursor(&mut self, c: char) -> Result<()> {
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
        self.selection.start_at(self.cursor_x, self.cursor_y);
    }
    
    fn update_cursor_to_selection_corner(&mut self) {
        if self.selection.active {
            // Position cursor at the bottom-right corner of selection
            let ((_, _), (max_x, max_y)) = self.selection.normalized();
            self.cursor_x = max_x;
            self.cursor_y = max_y;
        }
    }
    
    fn copy_selection(&mut self) -> Result<()> {
        if !self.selection.active {
            return Ok(());
        }
        
        // Use the Selection struct's extract_text method
        let clipboard_text = self.selection.extract_text(&self.text_buffer);
        let clipboard_lines: Vec<String> = clipboard_text.lines().map(|s| s.to_string()).collect();
        
        // Copy to both internal and system clipboard
        self.clipboard = clipboard_lines;
        
        // Copy to system clipboard
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(clipboard_text);
        }
        
        Ok(())
    }
    
    fn cut_selection(&mut self) -> Result<()> {
        if !self.selection.active {
            return Ok(());
        }
        
        // First copy the selection
        self.copy_selection()?;
        
        // Then delete the selected content
        self.delete_selection()?;
        
        Ok(())
    }
    
    fn delete_selection(&mut self) -> Result<()> {
        if !self.selection.active {
            return Ok(());
        }
        
        let mut lines: Vec<String> = self.text_buffer.lines().map(|s| s.to_string()).collect();
        
        // Normalize selection bounds
        let ((start_x, start_y), (end_x, end_y)) = self.selection.normalized();
        
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
        self.selection.clear();
        
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
        if let EditorMode::FilePicker { path, files, selected } = &mut self.mode {
            files.clear();
            
            // Add parent directory entry if not at root
            if let Some(parent) = path.parent() {
                if parent != path.as_path() {
                    let mut parent_path = parent.to_path_buf();
                    parent_path.push("..");
                    files.push(parent_path);
                }
            }
            
            // Read directory contents
            if let Ok(entries) = std::fs::read_dir(path) {
                let mut dirs = Vec::new();
                let mut pdf_files = Vec::new();
                
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        dirs.push(entry_path);
                    } else if let Some(ext) = entry_path.extension() {
                        if ext.to_string_lossy().to_lowercase() == "pdf" {
                            pdf_files.push(entry_path);
                        }
                    }
                }
                
                // Sort directories and files separately
                dirs.sort();
                pdf_files.sort();
                
                // Add to file list (directories first, then PDFs)
                files.extend(dirs);
                files.extend(pdf_files);
            }
            
            // Reset selection to first item
            *selected = 0;
        }
        
        Ok(())
    }
    
    fn render_file_picker(&self) -> Result<()> {
        let (path, files, selected) = match &self.mode {
            EditorMode::FilePicker { path, files, selected } => (path, files, selected),
            _ => return Ok(()),
        };
        
        TerminalRenderer::new()
            .clear_screen()
            .draw_text(0, 0, "📁 File Picker", Color::Cyan)
            .draw_text(0, 1, &format!("Current: {}", path.display()), Color::DarkGrey)
            .render();
        
        // File list
        let list_start = 3;
        let visible_lines = (self.terminal_height - 5) as usize; // Leave room for header and footer
        let start_index = if *selected >= visible_lines {
            *selected - visible_lines + 1
        } else {
            0
        };
        
        for (i, file_path) in files.iter().skip(start_index).take(visible_lines).enumerate() {
            let display_index = start_index + i;
            let y = list_start + i as u16;
            
            execute!(io::stdout(), cursor::MoveTo(0, y))?;
            
            let is_selected = display_index == *selected;
            let filename = file_path.file_name().unwrap_or_default().to_string_lossy();
            
            if is_selected {
                execute!(
                    io::stdout(),
                    SetBackgroundColor(Color::Blue),
                    SetForegroundColor(Color::White)
                )?;
            }
            
            // Icon and name
            if file_path.is_dir() || filename == ".." {
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
    
    
    fn handle_file_picker_input(&mut self, key: KeyCode, _modifiers: KeyModifiers) -> Result<bool> {
        let (path, files, selected) = match &mut self.mode {
            EditorMode::FilePicker { path, files, selected } => (path, files, selected),
            _ => return Ok(false),
        };
        
        match key {
            KeyCode::Up => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Down => {
                if *selected + 1 < files.len() {
                    *selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(selected_path) = files.get(*selected).cloned() {
                    if selected_path.file_name().unwrap_or_default() == ".." {
                        // Navigate to parent directory
                        if let Some(parent) = path.parent() {
                            *path = parent.to_path_buf();
                            self.scan_directory()?;
                        }
                    } else if selected_path.is_dir() {
                        // Navigate into directory
                        *path = selected_path;
                        self.scan_directory()?;
                    } else if selected_path.extension().map(|ext| ext.to_string_lossy().to_lowercase()) == Some("pdf".to_string()) {
                        // Load the PDF file
                        self.load_new_file(selected_path)?;
                        self.mode = EditorMode::Text { scroll_offset: 0 };
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.mode = EditorMode::Text { scroll_offset: 0 };
            }
            _ => {}
        }
        Ok(false)
    }
    
    fn load_new_file(&mut self, path: PathBuf) -> Result<()> {
        self.pdf_path = path;
        self.current_page = 1;
        self.total_pages = 1; // Will be updated in load_page
        // Reset any SplitScreen image rendering state
        if let EditorMode::SplitScreen { image_rendered, .. } = &mut self.mode {
            *image_rendered = false;
        }
        self.selection.clear();
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
            
            // Reset pan and force re-render if in SplitScreen mode
            if let EditorMode::SplitScreen { pan_offset, image_rendered, .. } = &mut self.mode {
                *pan_offset = (0, 0); // Reset pan when zoom changes
                *image_rendered = false; // Always force re-render at new resolution
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
            
            // Reset pan and force re-render if in SplitScreen mode
            if let EditorMode::SplitScreen { pan_offset, image_rendered, .. } = &mut self.mode {
                *pan_offset = (0, 0); // Reset pan when zoom changes
                *image_rendered = false; // Always force re-render at new resolution
                self.render_pdf_image()?;
            }
        }
        
        Ok(())
    }
    
    fn reset_zoom(&mut self) -> Result<()> {
        if self.zoom_level != 100 {
            self.zoom_level = 100;
            
            // Reset pan and force re-render if in SplitScreen mode
            if let EditorMode::SplitScreen { pan_offset, image_rendered, .. } = &mut self.mode {
                *pan_offset = (0, 0); // Reset pan when zoom resets
                *image_rendered = false; // Always force re-render at normal resolution
                self.render_pdf_image()?;
            }
        }
        
        Ok(())
    }
    
    
    
    
    fn with_history<F>(&mut self, f: F) -> Result<()> 
    where F: FnOnce(&mut Self) -> Result<()>
    {
        self.save_to_history();
        f(self)
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
        self.selection.clear();
        
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
        self.selection.clear();
        
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
        // Render PDF image if we're in SplitScreen mode and haven't rendered yet
        if let EditorMode::SplitScreen { image_rendered: false, .. } = &editor.mode {
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