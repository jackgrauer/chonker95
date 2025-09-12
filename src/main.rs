use anyhow::Result;
use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    /// PDF file to process
    file: PathBuf,
    
    /// Page number to extract (default: 1)
    #[arg(short, long, default_value_t = 1)]
    page: u32,
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
}

impl AltoElement {
    fn new(id: String, content: String, hpos: f32, vpos: f32, width: f32, height: f32) -> Self {
        let screen_x = (hpos / 8.0) as u16; // Convert PDF coords to terminal coords
        let screen_y = (vpos / 12.0) as u16;
        
        Self {
            _id: id,
            content,
            hpos,
            vpos,
            _width: width,
            _height: height,
            _screen_x: screen_x,
            _screen_y: screen_y,
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
            ab_mode: false,
            pdf_image_rendered: false,
        };
        
        editor.load_page()?;
        Ok(editor)
    }
    
    fn load_page(&mut self) -> Result<()> {
        self.elements = self.extract_alto_elements()?;
        self.rebuild_text_buffer();
        Ok(())
    }
    
    fn rebuild_text_buffer(&mut self) {
        // Sort elements by position to create readable text flow
        let mut sorted_elements = self.elements.clone();
        sorted_elements.sort_by(|a, b| {
            a.vpos.partial_cmp(&b.vpos)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.hpos.partial_cmp(&b.hpos).unwrap_or(std::cmp::Ordering::Equal))
        });
        
        // Build continuous text with intelligent line break preservation
        self.text_buffer = String::new();
        let mut last_vpos = 0.0;
        
        for element in &sorted_elements {
            // Calculate vertical gap from previous element
            let vpos_gap = element.vpos - last_vpos;
            
            // Determine how many line breaks are needed
            let line_breaks = if last_vpos == 0.0 {
                0 // First element
            } else if vpos_gap > 20.0 {
                2 // Large gap - paragraph break
            } else if vpos_gap > 12.0 {
                1 // Normal line break
            } else {
                0 // Same line - add space instead
            };
            
            // Add the determined line breaks
            for _ in 0..line_breaks {
                self.text_buffer.push('\n');
            }
            
            // Add horizontal spacing for same-line elements
            if line_breaks == 0 && !self.text_buffer.is_empty() {
                self.text_buffer.push(' ');
            }
            
            self.text_buffer.push_str(&element.content);
            last_vpos = element.vpos;
        }
    }
    
    fn extract_alto_elements(&self) -> Result<Vec<AltoElement>> {
        let output = std::process::Command::new("pdfalto")
            .args([
                "-f", &self.current_page.to_string(),
                "-l", &self.current_page.to_string(),
                "-readingOrder", "-noImage", "-noLineNumbers",
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
        
        // Show status line
        let mode_indicator = if self.ab_mode { "A-B COMPARE" } else { "TEXT EDIT" };
        execute!(
            io::stdout(),
            cursor::MoveTo(0, self.terminal_height - 1),
            SetForegroundColor(Color::Yellow),
            Print(format!("Chonker95 - {} - Page {} | {} | Cursor: {},{} | Ctrl+A toggle, Q quit", 
                self.pdf_path.file_name().unwrap_or_default().to_string_lossy(),
                self.current_page,
                mode_indicator,
                self.cursor_x,
                self.cursor_y)),
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
                execute!(
                    io::stdout(),
                    cursor::MoveTo(0, i as u16),
                    Print(line)
                )?;
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
        
        // Vertical separator
        for y in 0..(self.terminal_height - 1) {
            execute!(
                io::stdout(),
                cursor::MoveTo(split_x, y),
                SetForegroundColor(Color::Blue),
                Print("│"),
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
                execute!(
                    io::stdout(),
                    cursor::MoveTo(split_x + 1, i as u16),
                    Print(trimmed_line)
                )?;
            }
        }
        
        Ok(())
    }
    
    fn render_pdf_image(&mut self) -> Result<()> {
        if !self.ab_mode || self.pdf_image_rendered {
            return Ok(());
        }
        
        // Use ghostscript to render PDF page as grayscale JPG
        let output_file = format!("/tmp/chonker_kitty_{}.jpg", self.current_page);
        
        // Clean up any existing file first
        let _ = std::fs::remove_file(&output_file);
        
        // Debug: Show what command we're running
        eprintln!("DEBUG: Running ghostscript for page {} -> {}", self.current_page, output_file);
        
        let result = std::process::Command::new("gs")
            .args([
                "-dNOPAUSE", "-dBATCH", "-dSAFER", "-dQUIET",
                "-sDEVICE=jpeggray", // Grayscale JPEG
                &format!("-dFirstPage={}", self.current_page),
                &format!("-dLastPage={}", self.current_page),
                "-r150", // Good resolution for terminal display
                "-dJPEGQ=85", // High quality grayscale
                &format!("-sOutputFile={}", output_file),
            ])
            .arg(&self.pdf_path)
            .output()?;
            
        if !result.status.success() {
            eprintln!("DEBUG: Ghostscript failed: {}", String::from_utf8_lossy(&result.stderr));
            return Ok(());
        }
        
        if std::path::Path::new(&output_file).exists() {
            let file_size = std::fs::metadata(&output_file)?.len();
            eprintln!("DEBUG: JPG created: {} ({} bytes)", output_file, file_size);
            
            self.display_image_in_kitty(&output_file)?;
            self.pdf_image_rendered = true;
        } else {
            eprintln!("DEBUG: JPG file not created: {}", output_file);
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
        let cols = (split_x - 1).max(10);
        let rows = (self.terminal_height - 2).max(10);
        
        // Temporarily disable raw mode to display the image
        terminal::disable_raw_mode()?;
        
        // Clear the left panel area
        for y in 0..self.terminal_height {
            execute!(
                io::stdout(),
                cursor::MoveTo(0, y),
                Print(" ".repeat(split_x as usize))
            )?;
        }
        
        // Display the image using kitty icat
        let _ = std::process::Command::new("kitty")
            .args(["+kitten", "icat", "--clear", 
                   &format!("--place={}x{}@1x1", cols, rows),
                   image_path])
            .status();
        
        // Log what we did
        let _ = std::fs::write("/tmp/chonker_debug.log", 
            format!("Image displayed: {}\nDimensions: {}x{} at position 1,1\n", 
                   image_path, cols, rows));
        
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
            self.cursor_x = x;
            self.cursor_y = y;
            // No bounds checking - cursor can go anywhere, even off-screen
        }
        
        Ok(())
    }
    
    fn handle_key_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        match key {
            // Cursor movement
            KeyCode::Up => {
                if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                }
            }
            KeyCode::Down => {
                self.cursor_y += 1; // No bottom limit - can go beyond viewport
            }
            KeyCode::Left => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    // Ctrl+Left: Previous page
                    if self.current_page > 1 {
                        self.current_page -= 1;
                        self.load_page()?;
                    }
                } else if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            KeyCode::Right => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    // Ctrl+Right: Next page
                    self.current_page += 1;
                    self.load_page()?;
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
            
            // Global commands
            KeyCode::Char('a') | KeyCode::Char('A') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.ab_mode = !self.ab_mode;
                if self.ab_mode {
                    self.pdf_image_rendered = false; // Force re-render for A-B mode
                } else {
                    // Clear screen when exiting A-B mode
                    execute!(io::stdout(), Clear(ClearType::All))?;
                }
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(true),
            _ => {}
        }
        
        Ok(false)
    }
    
    fn insert_char_at_cursor(&mut self, c: char) -> Result<()> {
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