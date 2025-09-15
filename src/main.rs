use anyhow::Result;
use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor, SetBackgroundColor},
    terminal::{self, Clear, ClearType},
};
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use unicode_width::UnicodeWidthChar;

#[derive(Serialize, Deserialize, Debug)]
enum SyncMessage {
    PageChange(u32),
    CursorMove(u16, u16),
    TogglePanes,
    Quit,
}

struct SyncManager {
    socket_path: PathBuf,
}

impl SyncManager {
    fn new(pdf_path: &PathBuf) -> Self {
        let socket_name = format!("chonker95_{}.sock",
            pdf_path.file_stem().unwrap_or_default().to_string_lossy());
        let socket_path = std::env::temp_dir().join(socket_name);

        Self { socket_path }
    }

    fn send_message(&self, message: SyncMessage) -> Result<()> {
        if let Ok(stream) = std::os::unix::net::UnixStream::connect(&self.socket_path) {
            let json = serde_json::to_string(&message)?;
            use std::io::Write;
            let mut stream = stream;
            let _ = writeln!(stream, "{}", json);
        }
        Ok(())
    }

    fn start_listener(&self) -> Result<std::os::unix::net::UnixListener> {
        let _ = std::fs::remove_file(&self.socket_path); // Clean up old socket
        let listener = std::os::unix::net::UnixListener::bind(&self.socket_path)?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    }
}

// Kitty terminal detection and quirk handling
struct TerminalInfo {
    is_kitty: bool,
    is_mac: bool,
    supports_mouse: bool,
    version: String,
}

impl TerminalInfo {
    fn detect() -> Self {
        let term = std::env::var("TERM").unwrap_or_default();
        let kitty_window = std::env::var("KITTY_WINDOW_ID").is_ok();
        let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();

        let is_kitty = term.contains("kitty") ||
                      kitty_window ||
                      term_program.contains("kitty");

        let is_mac = cfg!(target_os = "macos");

        Self {
            is_kitty,
            is_mac,
            supports_mouse: true, // Most terminals support mouse now
            version: std::env::var("KITTY_VERSION").unwrap_or_default(),
        }
    }

    fn needs_special_handling(&self) -> bool {
        self.is_kitty
    }
}

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
    id: String,
    content: String,
    hpos: f32,
    vpos: f32,
    width: f32,
    height: f32,
    // Screen position (calculated from PDF coordinates)
    screen_x: u16,
    screen_y: u16,
}

impl AltoElement {
    fn new(id: String, content: String, hpos: f32, vpos: f32, width: f32, height: f32) -> Self {
        let screen_x = (hpos / 8.0) as u16; // Convert PDF coords to terminal coords
        let screen_y = (vpos / 12.0) as u16;
        
        Self {
            id,
            content,
            hpos,
            vpos,
            width,
            height,
            screen_x,
            screen_y,
        }
    }
}

// Display modes for the editor
#[derive(Debug, Clone, PartialEq)]
enum DisplayMode {
    TextOnly,
    SplitScreen,
}

// Mac-specific file operations
struct MacFileManager;

impl MacFileManager {
    // Move files to Mac Trash instead of permanent deletion
    fn move_to_trash(path: &PathBuf) -> Result<()> {
        trash::delete(path)?;
        Ok(())
    }

    // Get Mac-appropriate cache directory
    fn get_cache_dir() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("chonker95")
    }

    // Get Mac-appropriate documents directory
    fn get_documents_dir() -> PathBuf {
        dirs::document_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
    }

    // Get Mac-appropriate config directory
    fn get_config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")).join(".config"))
            .join("chonker95")
    }

    // Ensure directory exists with proper Mac permissions
    fn ensure_dir_exists(path: &PathBuf) -> Result<()> {
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
        Ok(())
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
    // Selection state
    is_all_selected: bool,
    clipboard: String,
    // Viewport scrolling for large documents
    viewport_offset_x: usize,
    viewport_offset_y: usize,
    // Full content grid (unlimited size)
    content_grid: Vec<Vec<char>>,
    grid_width: usize,
    grid_height: usize,
    // Terminal info for quirk handling
    terminal_info: TerminalInfo,
    // State to prevent ANSI hell
    terminal_state_clean: bool,
    // Display mode
    display_mode: DisplayMode,
    // Sync manager for pane communication
    sync_manager: SyncManager,
    // Mac-specific functionality
    file_manager: MacFileManager,
}

impl WysiwygEditor {
    fn new(pdf_path: PathBuf, page: u32) -> Result<Self> {
        let (width, height) = terminal::size()?;
        let terminal_info = TerminalInfo::detect();

        let sync_manager = SyncManager::new(&pdf_path);

        let mut editor = Self {
            elements: Vec::new(),
            pdf_path,
            current_page: page,
            terminal_width: width,
            terminal_height: height,
            text_buffer: String::new(),
            cursor_x: 0,
            cursor_y: 0,
            is_all_selected: false,
            clipboard: String::new(),
            viewport_offset_x: 0,
            viewport_offset_y: 0,
            content_grid: Vec::new(),
            grid_width: 0,
            grid_height: 0,
            terminal_info,
            terminal_state_clean: true,
            display_mode: DisplayMode::TextOnly,
            sync_manager,
            file_manager: MacFileManager,
        };

        // Initialize Mac-specific directories
        editor.init_mac_directories()?;

        editor.load_page()?;
        Ok(editor)
    }
    
    fn load_page(&mut self) -> Result<()> {
        self.elements = self.extract_alto_elements()?;
        self.rebuild_text_buffer();

        // Sync page with external viewer if in split-screen mode
        if self.display_mode == DisplayMode::SplitScreen {
            let _ = self.sync_external_viewer_page();
        }

        Ok(())
    }

    // Sync current page with external PDF viewer
    fn sync_external_viewer_page(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let filename = self.pdf_path.file_name()
                .unwrap_or_default()
                .to_string_lossy();

            // AppleScript to go to specific page in Skim/Preview
            let script = format!(
                r#"tell application "System Events"
                    try
                        tell application "Skim"
                            if exists (document whose name contains "{}") then
                                tell (document whose name contains "{}")
                                    go to page {}
                                end tell
                            end if
                        end tell
                    end try
                end tell"#,
                filename, filename, self.current_page
            );

            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .spawn();
        }

        Ok(())
    }


    // Toggle between text-only and split-screen mode
    // Initialize Mac-specific directories and settings
    fn init_mac_directories(&self) -> Result<()> {
        let cache_dir = MacFileManager::get_cache_dir();
        let config_dir = MacFileManager::get_config_dir();

        MacFileManager::ensure_dir_exists(&cache_dir)?;
        MacFileManager::ensure_dir_exists(&config_dir)?;

        Ok(())
    }

    // Save current session state to Mac config directory
    fn save_session_state(&self) -> Result<()> {
        let config_dir = MacFileManager::get_config_dir();
        let session_file = config_dir.join("last_session.json");

        let session_data = serde_json::json!({
            "last_file": self.pdf_path.to_string_lossy(),
            "current_page": self.current_page,
            "display_mode": match self.display_mode {
                DisplayMode::TextOnly => "text_only",
                DisplayMode::SplitScreen => "split_screen",
            },
            "terminal_size": {
                "width": self.terminal_width,
                "height": self.terminal_height
            }
        });

        std::fs::write(session_file, session_data.to_string())?;
        Ok(())
    }

    // Save extracted text to Mac Documents directory
    fn save_extracted_text(&self) -> Result<()> {
        let docs_dir = MacFileManager::get_documents_dir();
        MacFileManager::ensure_dir_exists(&docs_dir)?;

        let filename = self.pdf_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();

        let output_file = docs_dir.join(format!("{}_page_{}_extracted.txt", filename, self.current_page));

        std::fs::write(&output_file, &self.text_buffer)?;

        // Show brief confirmation in status (would need status message system)
        eprintln!("Saved extracted text to: {}", output_file.display());

        Ok(())
    }


    fn toggle_zellij_pane(&mut self) -> Result<()> {
        if Self::is_in_zellij() {
            // Check if we have multiple panes by trying to focus another pane
            let focus_result = std::process::Command::new("zellij")
                .arg("action")
                .arg("move-focus")
                .arg("left")
                .output();

            if focus_result.is_ok() {
                // We have multiple panes - close the one we just focused to
                std::process::Command::new("zellij")
                    .arg("action")
                    .arg("close-pane")
                    .spawn()?;
                self.display_mode = DisplayMode::TextOnly;
            } else {
                // Only one pane - create new viuer pane
                std::process::Command::new("zellij")
                    .arg("action")
                    .arg("new-pane")
                    .arg("--direction")
                    .arg("left")
                    .arg("--")
                    .arg("/Users/jack/chonker95/viuer-pane.sh")
                    .arg(&self.pdf_path)
                    .spawn()?;
                self.display_mode = DisplayMode::SplitScreen;
            }
        }
        Ok(())
    }


    // Check if running inside Zellij and spawn layout if not
    fn is_in_zellij() -> bool {
        std::env::var("ZELLIJ").is_ok() || std::env::var("ZELLIJ_SESSION_NAME").is_ok()
    }

    // Open PDF in external viewer as sidecar
    fn open_sidecar_pdf_viewer(&self) -> Result<()> {
        // If we're in Zellij, create new pane with zathura PDF viewer
        if Self::is_in_zellij() {
            // Create new pane in Zellij with zathura
            std::process::Command::new("zellij")
                .arg("action")
                .arg("new-pane")
                .arg("--direction")
                .arg("right")
                .arg("--")
                .arg("zathura")
                .arg(&self.pdf_path)
                .spawn()?;
        } else {
            // Not in Zellij - launch new Zellij session with layout
            let layout_path = std::env::current_dir()?.join("chonker95.kdl");

            if layout_path.exists() {
                std::process::Command::new("zellij")
                    .arg("--layout")
                    .arg(&layout_path)
                    .arg("--")
                    .arg(&self.pdf_path)
                    .spawn()?;
            } else {
                // Fallback to zathura directly
                std::process::Command::new("zathura")
                    .arg(&self.pdf_path)
                    .spawn()
                    .or_else(|_| {
                        // If zathura fails, try other viewers
                        #[cfg(target_os = "macos")]
                        {
                            std::process::Command::new("open")
                                .arg("-a")
                                .arg("Preview")
                                .arg(&self.pdf_path)
                                .spawn()
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            let viewers = ["evince", "okular", "xpdf"];
                            for viewer in &viewers {
                                if let Ok(child) = std::process::Command::new(viewer)
                                    .arg(&self.pdf_path)
                                    .spawn()
                                {
                                    return Ok(child);
                                }
                            }
                            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "No PDF viewer found"))
                        }
                    })?;
            }
        }

        Ok(())
    }

    // Close external PDF viewer (best effort)
    fn close_sidecar_pdf_viewer(&self) -> Result<()> {
        if Self::is_in_zellij() {
            // Close the PDF pane in Zellij
            std::process::Command::new("zellij")
                .arg("action")
                .arg("close-pane")
                .spawn()?;
        } else {
            #[cfg(target_os = "macos")]
            {
                let filename = self.pdf_path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy();

                let script = format!(
                    r#"tell application "System Events"
                        try
                            tell process "Preview"
                                close (every window whose name contains "{}")
                            end tell
                        end try
                    end tell"#,
                    filename
                );

                let _ = std::process::Command::new("osascript")
                    .arg("-e")
                    .arg(&script)
                    .spawn();
            }
        }

        Ok(())
    }

    
    fn rebuild_text_buffer(&mut self) {
        if self.elements.is_empty() {
            self.text_buffer = String::new();
            return;
        }

        // Build unlimited spatial grid and set viewport
        self.text_buffer = self.render_spatial_grid();
    }

    fn render_spatial_grid(&mut self) -> String {
        if self.elements.is_empty() {
            return String::new();
        }

        // Find content bounds to determine required grid size
        let min_x = self.elements.iter().map(|e| e.hpos).fold(f32::INFINITY, f32::min);
        let max_x = self.elements.iter().map(|e| e.hpos + e.width).fold(f32::NEG_INFINITY, f32::max);
        let min_y = self.elements.iter().map(|e| e.vpos).fold(f32::INFINITY, f32::min);
        let max_y = self.elements.iter().map(|e| e.vpos + e.height).fold(f32::NEG_INFINITY, f32::max);

        let content_width = (max_x - min_x).max(1.0);
        let content_height = (max_y - min_y).max(1.0);

        // Create unlimited grid based on actual content size
        let scale_factor = 0.15; // Character scaling from PDF coordinates
        self.grid_width = ((content_width * scale_factor) as usize + 50).max(300); // Minimum 300 cols
        self.grid_height = ((content_height * scale_factor) as usize + 20).max(100); // Minimum 100 rows

        self.content_grid = vec![vec![' '; self.grid_width]; self.grid_height];

        // Place each element in the unlimited grid
        for element in &self.elements {
            let grid_x = ((element.hpos - min_x) * scale_factor) as usize;
            let grid_y = ((element.vpos - min_y) * scale_factor) as usize;

            // Place each character with proper Unicode width handling
            let mut current_x = grid_x;
            for ch in element.content.chars() {
                let char_width = ch.width().unwrap_or(1);

                // Check if character fits in grid
                if current_x + char_width > self.grid_width {
                    break;
                }

                let final_y = grid_y.min(self.grid_height - 1);

                // Only place if grid position is empty (avoid overlaps)
                if current_x < self.grid_width && final_y < self.grid_height && self.content_grid[final_y][current_x] == ' ' {
                    self.content_grid[final_y][current_x] = ch;

                    // Wide characters (CJK, emoji) need to mark next cell as occupied
                    if char_width == 2 && current_x + 1 < self.grid_width {
                        self.content_grid[final_y][current_x + 1] = '​'; // Zero-width space to mark continuation
                    }
                }

                current_x += char_width;
            }
        }

        // Return viewport window of the large grid
        self.get_viewport_text()
    }

    fn get_viewport_text(&self) -> String {
        // Extract viewport window from the large content grid
        let viewport_width = (self.terminal_width as usize).min(120);
        let viewport_height = (self.terminal_height as usize - 2).min(50); // Reserve space for status

        let mut result = String::new();

        for screen_y in 0..viewport_height {
            let grid_y = screen_y + self.viewport_offset_y;

            if grid_y < self.grid_height {
                let mut line = String::new();

                for screen_x in 0..viewport_width {
                    let grid_x = screen_x + self.viewport_offset_x;

                    if grid_x < self.grid_width {
                        line.push(self.content_grid[grid_y][grid_x]);
                    } else {
                        line.push(' ');
                    }
                }

                result.push_str(&line.trim_end());
                result.push('\n');
            } else {
                result.push('\n');
            }
        }

        result.trim_end().to_string()
    }
    
    fn extract_alto_elements(&self) -> Result<Vec<AltoElement>> {
        // Extract ONLY the current page to avoid overlay issues
        let pdfium = Pdfium::default();
        let document = pdfium.load_pdf_from_file(&self.pdf_path, None)?;

        // Ensure we're only getting the specified page
        let page_index = (self.current_page - 1) as u16;
        if page_index >= document.pages().len() {
            return Ok(vec![]); // Page doesn't exist
        }

        let page = document.pages().get(page_index)?;
        let text_page = page.text()?;

        let mut elements = Vec::new();
        let page_height = page.height().value;
        let page_width = page.width().value;

        // For now, use segments which should work with the API
        let mut segment_elements = Vec::new();

        for (segment_idx, segment) in text_page.segments().iter().enumerate() {
            let segment_text = segment.text();
            let bounds = segment.bounds();

            // Split segment into words while preserving positioning
            let mut word_offset = 0.0;
            let chars_in_segment = segment_text.len() as f32;
            let avg_char_width = if chars_in_segment > 0.0 {
                bounds.width().value / chars_in_segment
            } else {
                6.0
            };

            for word in segment_text.split_whitespace() {
                if !word.is_empty() {
                    segment_elements.push(AltoElement::new(
                        format!("seg_{}_{}", segment_idx, segment_elements.len()),
                        word.to_string(),
                        bounds.left().value + word_offset,
                        page_height - bounds.top().value, // Flip Y coordinate
                        word.len() as f32 * avg_char_width,
                        bounds.height().value,
                    ));

                    word_offset += (word.len() + 1) as f32 * avg_char_width;
                }
            }
        }

        // Use segments if available, otherwise fallback
        let char_positions = segment_elements;

        // Return the elements from segments
        elements = char_positions;

        // Fallback if character extraction fails
        if elements.is_empty() {
            let text_content = page.text()?.all();
            let mut y_pos = 0.0;

            for line in text_content.lines() {
                let mut x_pos = 0.0;
                for word in line.split_whitespace() {
                    if !word.is_empty() {
                        elements.push(AltoElement::new(
                            format!("word_{}", elements.len()),
                            word.to_string(),
                            x_pos * 8.0,
                            y_pos * 12.0,
                            word.len() as f32 * 8.0,
                            12.0,
                        ));
                        x_pos += word.len() as f32 + 1.0;
                    }
                }
                y_pos += 1.0;
            }
        }

        Ok(elements)
    }

    
    
    // Enhanced terminal state management to prevent ANSI hell
    fn ensure_clean_state(&mut self) -> Result<()> {
        if !self.terminal_state_clean {
            // Reset all terminal attributes and clear any corrupted state
            execute!(
                io::stdout(),
                ResetColor,
                cursor::Show,
                Clear(ClearType::All)
            )?;
            self.terminal_state_clean = true;
        }
        Ok(())
    }

    fn render(&mut self) -> Result<()> {
        // Ensure clean state before every render to prevent ANSI hell
        self.ensure_clean_state()?;

        execute!(io::stdout(), Clear(ClearType::All), cursor::MoveTo(0, 0))?;

        // Always just render text - Zellij handles the pane management
        self.render_text_only()?;

        // Show status line with selection info
        let selection_info = if self.is_all_selected {
            " | ALL SELECTED"
        } else {
            ""
        };

        let mode_info = match self.display_mode {
            DisplayMode::TextOnly => "",
            DisplayMode::SplitScreen => " | PDF OPEN",
        };

        // Enhanced status line with Kitty-specific info
        let terminal_info = if self.terminal_info.is_kitty {
            format!(" KITTY v{}", self.terminal_info.version)
        } else {
            String::new()
        };

        let cmd_a_text = match self.display_mode {
            DisplayMode::TextOnly => "A:open-pdf",
            DisplayMode::SplitScreen => "A:close-pdf",
        };

        execute!(
            io::stdout(),
            cursor::MoveTo(0, self.terminal_height - 1),
            SetForegroundColor(Color::Yellow),
            Print(format!("Chonker95{} - {} - Page {}{}{} | {} | Cmd+{} S:save W:close Q:quit",
                terminal_info,
                self.pdf_path.file_name().unwrap_or_default().to_string_lossy(),
                self.current_page,
                mode_info,
                selection_info,
                self.get_mac_shortcut_text(),
                cmd_a_text)),
            ResetColor
        )?;

        // Position cursor with extra safety for Kitty
        let cursor_x = self.cursor_x;

        execute!(
            io::stdout(),
            cursor::MoveTo(cursor_x, self.cursor_y),
            cursor::Show
        )?;

        io::stdout().flush()?;
        Ok(())
    }
    
    fn render_text_only(&self) -> Result<()> {
        // Display the text buffer with selection highlighting
        let lines: Vec<&str> = self.text_buffer.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if i < (self.terminal_height - 2) as usize {
                execute!(io::stdout(), cursor::MoveTo(0, i as u16))?;

                if self.is_all_selected {
                    // Highlight entire line if everything is selected
                    execute!(
                        io::stdout(),
                        SetBackgroundColor(Color::Blue),
                        SetForegroundColor(Color::White),
                        Print(line),
                        ResetColor
                    )?;
                } else {
                    execute!(io::stdout(), Print(line))?;
                }
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
    
    // Enhanced key handling with Kitty-specific quirks
    fn normalize_key_for_terminal(&self, key: KeyCode, modifiers: KeyModifiers) -> (KeyCode, KeyModifiers) {
        if self.terminal_info.is_kitty {
            // Kitty-specific key normalization
            match (key, modifiers) {
                // Kitty sometimes sends different modifier combinations
                (KeyCode::Char(c), mods) if mods.contains(KeyModifiers::ALT) && mods.contains(KeyModifiers::SHIFT) => {
                    // Normalize Alt+Shift combinations that Kitty might mangle
                    (KeyCode::Char(c), KeyModifiers::ALT)
                },
                // Handle Kitty's special key reporting
                (KeyCode::Left, mods) if mods.contains(KeyModifiers::CONTROL) && mods.contains(KeyModifiers::SHIFT) => {
                    (KeyCode::Left, KeyModifiers::CONTROL)
                },
                (KeyCode::Right, mods) if mods.contains(KeyModifiers::CONTROL) && mods.contains(KeyModifiers::SHIFT) => {
                    (KeyCode::Right, KeyModifiers::CONTROL)
                },
                _ => (key, modifiers),
            }
        } else {
            (key, modifiers)
        }
    }

    fn handle_key_input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
        // Mark state as potentially dirty after key input
        self.terminal_state_clean = false;

        // Normalize key for terminal-specific quirks
        let (normalized_key, normalized_modifiers) = self.normalize_key_for_terminal(key, modifiers);

        match normalized_key {
            // Cursor movement
            KeyCode::Up => {
                if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                }
            }
            KeyCode::Down => {
                self.cursor_y += 1; // No bottom limit - can go beyond viewport
            }
            // Mac Home/End support (check Mac modifiers first)
            KeyCode::Left if self.is_mac_modifier(normalized_modifiers) => {
                self.cursor_x = 0; // Beginning of line
            }
            KeyCode::Right if self.is_mac_modifier(normalized_modifiers) => {
                // End of line
                let lines: Vec<&str> = self.text_buffer.lines().collect();
                if let Some(current_line) = lines.get(self.cursor_y as usize) {
                    self.cursor_x = current_line.len() as u16;
                } else {
                    self.cursor_x = 0;
                }
            }
            // Viewport scrolling controls
            KeyCode::Left if normalized_modifiers.contains(KeyModifiers::ALT) => {
                self.viewport_offset_x = self.viewport_offset_x.saturating_sub(10);
            }
            KeyCode::Right if normalized_modifiers.contains(KeyModifiers::ALT) => {
                self.viewport_offset_x = (self.viewport_offset_x + 10).min(self.grid_width.saturating_sub(50));
            }
            KeyCode::Left if normalized_modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+Left: Previous page
                if self.current_page > 1 {
                    self.current_page -= 1;
                    self.load_page()?;
                    // Sync page change to other pane
                    let _ = self.sync_manager.send_message(SyncMessage::PageChange(self.current_page));
                }
            }
            KeyCode::Right if normalized_modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl+Right: Next page
                self.current_page += 1;
                self.load_page()?;
                // Sync page change to other pane
                let _ = self.sync_manager.send_message(SyncMessage::PageChange(self.current_page));
            }
            KeyCode::Left => {
                if self.cursor_x > 0 {
                    // Move left by one display column (Unicode-aware)
                    let lines: Vec<&str> = self.text_buffer.lines().collect();
                    if let Some(line) = lines.get(self.cursor_y as usize) {
                        let mut visual_col = 0;
                        let mut prev_visual_col = 0;

                        for ch in line.chars() {
                            let char_width = ch.width().unwrap_or(1);
                            if visual_col >= self.cursor_x as usize {
                                self.cursor_x = prev_visual_col as u16;
                                break;
                            }
                            prev_visual_col = visual_col;
                            visual_col += char_width;
                        }

                        if visual_col < self.cursor_x as usize {
                            self.cursor_x = (self.cursor_x - 1).max(0);
                        }
                    } else {
                        self.cursor_x -= 1;
                    }
                }
            }
            KeyCode::Right => {
                // Move right by one display column (Unicode-aware)
                let lines: Vec<&str> = self.text_buffer.lines().collect();
                if let Some(line) = lines.get(self.cursor_y as usize) {
                    let mut visual_col = 0;

                    for ch in line.chars() {
                        if visual_col == self.cursor_x as usize {
                            let char_width = ch.width().unwrap_or(1);
                            self.cursor_x += char_width as u16;
                            return Ok(false);
                        }
                        visual_col += ch.width().unwrap_or(1);
                    }
                }
                // Default: just move one column if no special character
                self.cursor_x += 1;
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
            // Toggle Zellij pane with Ctrl+P (handle BEFORE text input)
            KeyCode::Char('p') | KeyCode::Char('P') if normalized_modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_zellij_pane()?;
            }

            // Text editing (Mac-aware)
            KeyCode::Char(c) if !self.is_mac_modifier(normalized_modifiers) && !normalized_modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_char_at_cursor(c)?;
            }
            KeyCode::Backspace => {
                self.delete_char_at_cursor()?;
            }
            KeyCode::Enter => {
                self.insert_char_at_cursor('\n')?;
            }
            KeyCode::Char('c') | KeyCode::Char('C') if self.is_mac_modifier(normalized_modifiers) => {
                self.copy_selection()?;
            }
            KeyCode::Char('x') | KeyCode::Char('X') if self.is_mac_modifier(normalized_modifiers) => {
                self.cut_selection()?;
            }
            KeyCode::Char('v') | KeyCode::Char('V') if self.is_mac_modifier(normalized_modifiers) => {
                self.paste_from_clipboard()?;
            }
            KeyCode::Esc => {
                self.clear_selection();
            }

            // Additional viewport scrolling
            KeyCode::PageUp => {
                self.viewport_offset_y = self.viewport_offset_y.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.viewport_offset_y = (self.viewport_offset_y + 10).min(self.grid_height.saturating_sub(20));
            }

            // Mac-specific file operations
            KeyCode::Char('s') | KeyCode::Char('S') if self.is_mac_modifier(normalized_modifiers) => {
                self.save_extracted_text()?;
            }
            KeyCode::Char('o') | KeyCode::Char('O') if self.is_mac_modifier(normalized_modifiers) => {
                // TODO: Implement file picker for opening new PDFs
                // For now, just save session state
                let _ = self.save_session_state();
            }
            KeyCode::Char('w') | KeyCode::Char('W') if self.is_mac_modifier(normalized_modifiers) => {
                // Mac convention: Cmd+W closes window, Cmd+Q quits app
                return Ok(true);
            }
            KeyCode::Char('q') | KeyCode::Char('Q') if self.is_mac_modifier(normalized_modifiers) => return Ok(true),
            _ => {}
        }
        
        Ok(false)
    }
    
    fn insert_char_at_cursor(&mut self, c: char) -> Result<()> {
        // Clear selection when editing
        self.clear_selection();

        let mut lines: Vec<String> = self.text_buffer.lines().map(|s| s.to_string()).collect();

        // Ensure we have enough lines for cursor position
        while lines.len() <= self.cursor_y as usize {
            lines.push(String::new());
        }

        // Get the current line, extending it if cursor is beyond text
        let current_line = &mut lines[self.cursor_y as usize];

        // Extend line with spaces if cursor is beyond current text (character-based, not byte-based)
        while current_line.chars().count() < self.cursor_x as usize {
            current_line.push(' ');
        }

        // Convert cursor position to safe byte index for UTF-8
        let safe_cursor_pos = {
            let mut char_count = 0;
            let mut byte_pos = 0;
            for (i, _) in current_line.char_indices() {
                if char_count >= self.cursor_x as usize {
                    byte_pos = i;
                    break;
                }
                char_count += 1;
            }
            // If cursor is at end, use string length
            if char_count < self.cursor_x as usize {
                current_line.len()
            } else {
                byte_pos
            }
        };

        if c == '\n' {
            // Split line at cursor - UTF-8 safe
            let remaining = if safe_cursor_pos < current_line.len() {
                current_line.split_off(safe_cursor_pos)
            } else {
                String::new()
            };
            lines.insert(self.cursor_y as usize + 1, remaining);
            self.cursor_y += 1;
            self.cursor_x = 0;
        } else {
            // Insert character at safe UTF-8 position
            let insert_pos = safe_cursor_pos.min(current_line.len());
            current_line.insert(insert_pos, c);
            self.cursor_x += 1;
        }

        // Rebuild text buffer
        self.text_buffer = lines.join("\n");
        Ok(())
    }
    
    fn delete_char_at_cursor(&mut self) -> Result<()> {
        // Clear selection when editing
        self.clear_selection();

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

    fn select_all(&mut self) {
        // Select all text content
        self.is_all_selected = true;
    }

    fn clear_selection(&mut self) {
        // Clear any selection
        self.is_all_selected = false;
    }

    fn copy_selection(&mut self) -> Result<()> {
        if self.is_all_selected {
            // Copy all text to internal clipboard
            self.clipboard = self.text_buffer.clone();

            // Copy to system clipboard so other apps can access it
            if let Ok(mut system_clipboard) = arboard::Clipboard::new() {
                let _ = system_clipboard.set_text(&self.text_buffer);
            }
        }
        Ok(())
    }

    fn cut_selection(&mut self) -> Result<()> {
        if self.is_all_selected {
            // Copy to clipboard first
            self.copy_selection()?;

            // Then clear all text
            self.text_buffer = String::new();
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.clear_selection();
        }
        Ok(())
    }

    fn paste_from_clipboard(&mut self) -> Result<()> {
        // Clear selection when pasting
        self.clear_selection();

        // Try to get text from system clipboard first, fallback to internal
        let clipboard_text = if let Ok(mut system_clipboard) = arboard::Clipboard::new() {
            system_clipboard.get_text().unwrap_or_else(|_| self.clipboard.clone())
        } else {
            self.clipboard.clone()
        };

        if !clipboard_text.is_empty() {
            // Insert clipboard content at cursor position
            let lines: Vec<String> = self.text_buffer.lines().map(|s| s.to_string()).collect();
            let mut new_lines = lines;

            // Ensure we have enough lines for cursor position
            while new_lines.len() <= self.cursor_y as usize {
                new_lines.push(String::new());
            }

            // Get current line and split at cursor - SAFE UTF-8 handling
            let current_line = &mut new_lines[self.cursor_y as usize];

            // Convert cursor position to safe byte index for UTF-8
            let safe_cursor_pos = {
                let mut char_count = 0;
                let mut byte_pos = 0;
                for (i, _) in current_line.char_indices() {
                    if char_count >= self.cursor_x as usize {
                        break;
                    }
                    byte_pos = i;
                    char_count += 1;
                }
                // If cursor is beyond string, use string length
                if char_count < self.cursor_x as usize {
                    current_line.len()
                } else {
                    byte_pos
                }
            };

            // Extend line with spaces if cursor is beyond current text
            while current_line.chars().count() < self.cursor_x as usize {
                current_line.push(' ');
            }

            // Insert clipboard content
            let clipboard_lines: Vec<&str> = clipboard_text.lines().collect();

            if clipboard_lines.len() <= 1 {
                // Single line paste - insert at safe cursor position
                let insert_pos = safe_cursor_pos.min(current_line.len());
                current_line.insert_str(insert_pos, &clipboard_lines.get(0).unwrap_or(&""));
                self.cursor_x += clipboard_lines.get(0).unwrap_or(&"").chars().count() as u16;
            } else {
                // Multi-line paste - split current line safely
                let safe_split_pos = safe_cursor_pos.min(current_line.len());
                let remaining_text = if safe_split_pos < current_line.len() {
                    current_line.split_off(safe_split_pos)
                } else {
                    String::new()
                };

                current_line.push_str(&clipboard_lines[0]);

                // Insert middle lines
                if clipboard_lines.len() > 2 {
                    for (i, line) in clipboard_lines[1..clipboard_lines.len()-1].iter().enumerate() {
                        new_lines.insert(self.cursor_y as usize + i + 1, line.to_string());
                    }
                }

                // Insert last line with remaining text
                let last_line = format!("{}{}",
                    clipboard_lines.last().unwrap_or(&""),
                    remaining_text
                );
                new_lines.insert(
                    self.cursor_y as usize + clipboard_lines.len().saturating_sub(1),
                    last_line
                );

                self.cursor_y += clipboard_lines.len().saturating_sub(1) as u16;
                self.cursor_x = clipboard_lines.last().unwrap_or(&"").chars().count() as u16;
            }

            self.text_buffer = new_lines.join("\n");
        }

        Ok(())
    }

    fn get_mac_shortcut_text(&self) -> &'static str {
        // Show correct shortcuts in status bar
        #[cfg(target_os = "macos")]
        {
            "Cmd+A/C/X/V"
        }
        #[cfg(not(target_os = "macos"))]
        {
            "Ctrl+A/C/X/V"
        }
    }

    fn is_mac_modifier(&self, modifiers: KeyModifiers) -> bool {
        // Mac users expect Cmd key, others use Ctrl
        #[cfg(target_os = "macos")]
        {
            modifiers.contains(KeyModifiers::SUPER) // Cmd key on Mac
        }
        #[cfg(not(target_os = "macos"))]
        {
            modifiers.contains(KeyModifiers::CONTROL) // Ctrl key on others
        }
    }
}

// Enhanced terminal setup with Kitty-specific handling
fn setup_terminal(terminal_info: &TerminalInfo) -> Result<()> {
    // Enable raw mode with extra error handling
    terminal::enable_raw_mode().map_err(|e| {
        eprintln!("Failed to enable raw mode: {}", e);
        e
    })?;

    // Enhanced terminal setup for Kitty
    if terminal_info.is_kitty {
        // Kitty-specific setup to prevent conflicts
        execute!(
            io::stdout(),
            terminal::EnterAlternateScreen,
            event::EnableMouseCapture,
            cursor::Hide,
            // Send additional escape sequences to ensure clean state
            Print("\x1b[?1049h"), // Alternative screen buffer
            Print("\x1b[2J"),      // Clear screen
            Print("\x1b[H"),       // Move cursor to home
            Clear(ClearType::All)
        )?;
    } else {
        // Standard setup for other terminals
        execute!(
            io::stdout(),
            terminal::EnterAlternateScreen,
            event::EnableMouseCapture,
            cursor::Hide,
            Clear(ClearType::All)
        )?;
    }

    Ok(())
}

fn cleanup_terminal(terminal_info: &TerminalInfo) -> Result<()> {
    // Enhanced cleanup to prevent ANSI hell on exit
    if terminal_info.is_kitty {
        execute!(
            io::stdout(),
            cursor::Show,
            ResetColor,
            Print("\x1b[?1049l"), // Exit alternative screen buffer
            event::DisableMouseCapture,
            terminal::LeaveAlternateScreen
        )?;
    } else {
        execute!(
            io::stdout(),
            cursor::Show,
            ResetColor,
            event::DisableMouseCapture,
            terminal::LeaveAlternateScreen
        )?;
    }

    terminal::disable_raw_mode()?;
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Detect terminal early for proper setup
    let terminal_info = TerminalInfo::detect();

    // Set panic hook to cleanup terminal on panic
    let terminal_info_clone = TerminalInfo::detect();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Emergency terminal cleanup on panic
        let _ = execute!(
            io::stdout(),
            cursor::Show,
            ResetColor,
            event::DisableMouseCapture,
            terminal::LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();

        eprintln!("PANIC occurred: {}", panic_info);
        eprintln!("Terminal has been restored to normal mode.");
        eprintln!("This was likely caused by invalid UTF-8 string operations.");
    }));

    // Setup terminal with proper error handling
    if let Err(e) = setup_terminal(&terminal_info) {
        eprintln!("Terminal setup failed: {}", e);
        eprintln!("This might be due to terminal compatibility issues.");
        eprintln!("Try running in a different terminal or check your TERM environment variable.");
        return Err(e);
    }

    let result = {
        let mut editor = WysiwygEditor::new(cli.file, cli.page)?;

        loop {
            match editor.render() {
                Ok(()) => {},
                Err(e) => {
                    eprintln!("Render error: {}", e);
                    editor.terminal_state_clean = false;
                    continue;
                }
            }

            match event::read() {
                Ok(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: x,
                    row: y,
                    ..
                })) => {
                    if let Err(e) = editor.handle_mouse_click(x, y) {
                        eprintln!("Mouse handling error: {}", e);
                        editor.terminal_state_clean = false;
                    }
                }
                Ok(Event::Key(key_event)) => {
                    match editor.handle_key_input(key_event.code, key_event.modifiers) {
                        Ok(true) => {
                            // Save session state before quitting
                            let _ = editor.save_session_state();
                            break; // Quit requested
                        }
                        Ok(false) => {}, // Continue
                        Err(e) => {
                            eprintln!("Key handling error: {}", e);
                            editor.terminal_state_clean = false;
                        }
                    }
                }
                Ok(_) => {} // Other events
                Err(e) => {
                    eprintln!("Event read error: {}", e);
                    editor.terminal_state_clean = false;
                }
            }
        }

        Ok(())
    };

    // Always cleanup terminal, even on error
    if let Err(cleanup_err) = cleanup_terminal(&terminal_info) {
        eprintln!("Terminal cleanup failed: {}", cleanup_err);
    }

    result
}