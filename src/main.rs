use anyhow::Result;
use eframe;
use egui;
use ropey::Rope;
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct UnifiedAltoElement {
    content: String,
    hpos: f32,
    vpos: f32,
}

#[derive(Debug, Clone)]
struct Word {
    content: String,
    hpos: f32,
    vpos: f32,
}

#[derive(Debug, Clone)]
struct Line {
    words: Vec<Word>,
    avg_vpos: f32,
    is_table: bool,
}

impl UnifiedAltoElement {
    fn new(content: String, hpos: f32, vpos: f32) -> Self {
        Self { content, hpos, vpos }
    }
}

struct TerminalGrid {
    grid: Vec<Vec<char>>,
    grid_width: usize,
    grid_height: usize,
    char_width: f32,
    line_height: f32,
}

impl TerminalGrid {
    fn new() -> Self {
        Self::new_with_size(140, 60)
    }
    
    fn new_with_size(width: usize, height: usize) -> Self {
        Self {
            grid: vec![vec![' '; width]; height],
            grid_width: width,
            grid_height: height,
            char_width: 4.5,
            line_height: 9.0,
        }
    }
    
    fn alto_to_grid(&self, hpos: f32, vpos: f32) -> (usize, usize) {
        let col = (hpos / self.char_width) as usize;
        let row = (vpos / self.line_height) as usize;
        (col.min(self.grid_width - 1), row.min(self.grid_height - 1))
    }
    
    fn to_text(&self) -> String {
        let lines: Vec<String> = self.grid.iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect();
        
        // Find first and last non-empty lines
        let first_content = lines.iter().position(|line| !line.is_empty());
        let last_content = lines.iter().rposition(|line| !line.is_empty());
        
        match (first_content, last_content) {
            (Some(first), Some(last)) => {
                // Keep everything between first and last content, INCLUDING empty lines
                lines[first..=last].join("\n")
            }
            _ => String::new(),
        }
    }
}

struct EditorState {
    fake_scroll_x: f32,
    needs_repaint: bool,
    last_text_hash: u64,
    current_page: u32,
    total_pages: u32,
}

impl Default for EditorState {
    fn default() -> Self {
        Self { 
            fake_scroll_x: 0.0, 
            needs_repaint: true, 
            last_text_hash: 0,
            current_page: 1,
            total_pages: 2,
        }
    }
}

struct UnifiedAltoEditor {
    terminal_grid: TerminalGrid,
    grid_rope: Rope,
    grid_text_cache: Option<String>,
    rope_dirty: bool,
    state: EditorState,
}

impl UnifiedAltoEditor {
    fn new() -> Self {
        let mut editor = Self { 
            terminal_grid: TerminalGrid::new(), 
            grid_rope: Rope::new(),
            grid_text_cache: None,
            rope_dirty: true,
            state: EditorState::default() 
        };
        // Start with empty text editor
        editor.rebuild_grid(vec![]);
        editor
    }
    
    fn get_text_for_egui(&mut self) -> String {
        if self.rope_dirty || self.grid_text_cache.is_none() {
            let text = self.grid_rope.to_string();
            self.grid_text_cache = Some(text.clone());
            self.rope_dirty = false;
            text
        } else {
            self.grid_text_cache.as_ref().unwrap().clone()
        }
    }
    
    // Step 1: Word Extraction  
    fn extract_words(&self, elements: &[UnifiedAltoElement]) -> Vec<Word> {
        elements.iter()
            .map(|elem| Word {
                content: elem.content.clone(),
                hpos: elem.hpos,
                vpos: elem.vpos,
            })
            .collect()
    }
    
    // Step 2: Line Formation
    fn form_lines(&self, words: Vec<Word>) -> Vec<Line> {
        let mut lines_map = std::collections::BTreeMap::new();
        
        for word in words {
            let line_key = (word.vpos / 12.0) as i32; // Group by similar vpos
            lines_map.entry(line_key).or_insert_with(Vec::new).push(word);
        }
        
        lines_map.into_iter()
            .map(|(_, mut words)| {
                words.sort_by(|a, b| a.hpos.partial_cmp(&b.hpos).unwrap());
                let avg_vpos = words.iter().map(|w| w.vpos).sum::<f32>() / words.len() as f32;
                Line {
                    words,
                    avg_vpos,
                    is_table: false, // Will be set by detector
                }
            })
            .collect()
    }
    
    // Step 3: Table Detection Lever
    fn detect_tables(&self, lines: &mut [Line]) {
        for i in 0..lines.len() {
            if lines[i].words.len() < 2 { continue; }
            
            // Look for vertical alignment patterns
            let mut alignment_score = 0;
            let current_positions: Vec<f32> = lines[i].words.iter().map(|w| w.hpos).collect();
            
            // Check lines above and below for similar hpos patterns
            for j in 0..lines.len() {
                if i == j || lines[j].words.len() < 2 { continue; }
                
                let other_positions: Vec<f32> = lines[j].words.iter().map(|w| w.hpos).collect();
                
                // Count how many positions align (within tolerance)
                for &pos1 in &current_positions {
                    for &pos2 in &other_positions {
                        if (pos1 - pos2).abs() < 20.0 { // 20px tolerance
                            alignment_score += 1;
                        }
                    }
                }
            }
            
            // If we find enough alignments, mark as table
            lines[i].is_table = alignment_score > lines[i].words.len() * 2;
        }
    }
    
    // Step 4: Formatter Behavior
    fn format_line(&self, line: &Line) -> String {
        if !line.is_table {
            // Paragraph mode: natural spacing
            line.words.iter().map(|w| w.content.as_str()).collect::<Vec<_>>().join(" ")
        } else {
            // Table mode: coordinate-aligned spacing
            let mut result = String::new();
            let mut current_pos = 0.0;
            
            for (i, word) in line.words.iter().enumerate() {
                if i > 0 {
                    let spaces_needed = ((word.hpos - current_pos) / 4.5).max(1.0).min(20.0) as usize;
                    result.push_str(&" ".repeat(spaces_needed));
                }
                result.push_str(&word.content);
                current_pos = word.hpos + (word.content.len() as f32 * 4.5);
            }
            result
        }
    }

    fn rebuild_grid(&mut self, elements: Vec<UnifiedAltoElement>) {
        // Step 1: Extract words
        let words = self.extract_words(&elements);
        
        // Step 2: Form lines
        let mut lines = self.form_lines(words);
        
        // Step 3: Detect tables
        self.detect_tables(&mut lines);
        
        // Step 4 & 5: Format and assemble output with preserved spacing
        let mut formatted_text = String::new();
        
        if !lines.is_empty() {
            // Sort lines by their vertical position
            lines.sort_by(|a, b| a.avg_vpos.partial_cmp(&b.avg_vpos).unwrap());
            
            let mut last_vpos = lines[0].avg_vpos;
            
            for line in lines {
                // Calculate how many line breaks should exist based on vertical gap
                let vpos_gap = line.avg_vpos - last_vpos;
                let line_breaks_needed = (vpos_gap / 12.0).round() as i32; // 12pt = typical line height
                
                // Add empty lines for gaps (minimum 1, maximum 5 to prevent huge gaps)
                let empty_lines = line_breaks_needed.max(1).min(5) - 1;
                for _ in 0..empty_lines {
                    formatted_text.push('\n');
                }
                
                let formatted_line = self.format_line(&line);
                formatted_text.push_str(&formatted_line);
                formatted_text.push('\n');
                
                last_vpos = line.avg_vpos;
            }
        }
        
        // Update rope with formatted text
        self.grid_rope = Rope::from_str(&formatted_text);
        self.grid_text_cache = None;
        self.rope_dirty = true;
    }
    
    fn load_page(&mut self, page: u32) {
        self.state.current_page = page;
        // This method is kept for backward compatibility but should not be used
        // when a specific PDF path is available
        let elements = Self::create_fallback_elements();
        self.rebuild_grid(elements);
    }
    
    fn load_page_from_pdf(&mut self, pdf_path: &str, page: u32) {
        self.state.current_page = page;
        let elements = Self::load_alto_page_from_pdf(pdf_path, page);
        self.rebuild_grid(elements);
    }
    
    fn load_alto_page_from_pdf(pdf_path: &str, page: u32) -> Vec<UnifiedAltoElement> {
        // Extract all ALTO elements from PDF using our proven parsing
        
        std::process::Command::new("pdfalto")
            .args(["-f", &page.to_string(), "-l", &page.to_string(), "-readingOrder", "-noImage", "-noLineNumbers",
                   pdf_path, "/dev/stdout"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                let xml_data = String::from_utf8_lossy(&output.stdout);
                Self::parse_alto_elements(&xml_data)
            })
            .unwrap_or_else(Self::create_fallback_elements)
    }
    
    fn create_fallback_elements() -> Vec<UnifiedAltoElement> {
        vec![
            UnifiedAltoElement::new("DEMO".to_string(), 160.8, 84.8),
            UnifiedAltoElement::new("ALTO".to_string(), 200.0, 84.8),
            UnifiedAltoElement::new("EDITOR".to_string(), 240.0, 84.8),
            UnifiedAltoElement::new("No PDF loaded - demo mode".to_string(), 78.6, 110.0),
        ]
    }
    
    fn parse_alto_elements(xml: &str) -> Vec<UnifiedAltoElement> {
        use quick_xml::{Reader, events::Event};
        
        let mut elements = Vec::new();
        let mut reader = Reader::from_str(xml);
        let mut buf = Vec::new();
        
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.name().as_ref() == b"String" {
                        let mut content = String::new();
                        let mut hpos = 0.0;
                        let mut vpos = 0.0;
                        
                        for attr in e.attributes().with_checks(false) {
                            if let Ok(attr) = attr {
                                let key = String::from_utf8_lossy(attr.key.as_ref());
                                let value = String::from_utf8_lossy(&attr.value);
                                
                                match key.as_ref() {
                                    "CONTENT" => content = value.to_string(),
                                    "HPOS" => hpos = value.parse().unwrap_or(0.0),
                                    "VPOS" => vpos = value.parse().unwrap_or(0.0),
                                    _ => {}
                                }
                            }
                        }
                        
                        if !content.is_empty() {
                            elements.push(UnifiedAltoElement::new(content, hpos, vpos));
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
    
    fn show(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let input = ui.input(|i| i.clone());


        // HORIZONTAL SWIPE HACK: Only respond to horizontal scroll for left/right pan
        let mut scroll_changed = false;
        if input.raw_scroll_delta.x.abs() > input.raw_scroll_delta.y.abs() && input.raw_scroll_delta.x.abs() > 0.1 {
            let old_scroll = self.state.fake_scroll_x;
            self.state.fake_scroll_x += input.raw_scroll_delta.x;
            
            // IMPROVED BOUNDS: Content width vs panel width with safety margins
            let max_line_length = self.grid_rope.lines()
                .map(|line| line.len_chars())
                .max()
                .unwrap_or(140) as f32;
            let panel_width = ui.available_width();
            let content_width = max_line_length * 7.2; // Accurate character width
            
            // CLAMP WITH MARGINS: Prevent infinite pan but allow some overscroll
            let margin = 50.0;
            let min_scroll = (panel_width - content_width - margin).min(margin);
            let max_scroll = margin;
            
            self.state.fake_scroll_x = self.state.fake_scroll_x.clamp(min_scroll, max_scroll);
            scroll_changed = self.state.fake_scroll_x != old_scroll;
            
            if scroll_changed {
                self.state.needs_repaint = true; // Only repaint when scroll actually changes
            }
        }

        // SIMPLE SCROLLABLE TERMINAL GRID: Performance optimized
        if self.state.needs_repaint || scroll_changed {
            // Only request repaint when actually needed
            ui.ctx().request_repaint();
            self.state.needs_repaint = false;
        }
        
        egui::ScrollArea::both()
            .auto_shrink([false; 2])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden) // Hide scroll bars
            .show(ui, |ui| {
                // Calculate size based on grid dimensions
                let char_width = 7.2; // Monospace character width
                let line_height = 14.0; // Line height
                let content_width = self.terminal_grid.grid_width as f32 * char_width;
                let content_height = self.terminal_grid.grid_height as f32 * line_height;
                
                // Dramatically extend content width to prevent wrapping when zoomed
                let extended_width = content_width * 5.0; // Much wider to handle zoom
                
                let mut text_for_egui = self.get_text_for_egui();
                let response = ui.add_sized(
                    egui::vec2(extended_width, content_height),
                    egui::TextEdit::multiline(&mut text_for_egui)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(extended_width)
                        .frame(false)
                );
                
                // Update cache and rope if text was modified
                if response.changed() {
                    self.grid_rope = Rope::from_str(&text_for_egui);
                    self.grid_text_cache = Some(text_for_egui);
                    self.rope_dirty = false;
                }
                
                // THROTTLED GRID UPDATES: Only update when text actually changes
                if response.changed() {
                    // Inline hash calculation for performance
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    self.grid_rope.to_string().hash(&mut hasher);
                    let new_hash = hasher.finish();
                    
                    if new_hash != self.state.last_text_hash {
                        self.state.last_text_hash = new_hash;
                        self.state.needs_repaint = true;
                    }
                }
                
                response
            }).inner
    }
    
}

struct AltoApp {
    editor: UnifiedAltoEditor,
    // PDF display state
    pdf_path: Option<PathBuf>,
    current_page: usize,
    total_pages: usize,
    // Zoom state
    zoom_level: f32,
}

impl Default for AltoApp {
    fn default() -> Self {
        let editor = UnifiedAltoEditor::new();
        Self { 
            editor,
            pdf_path: None,
            current_page: 1,
            total_pages: 0,
            zoom_level: 2.0,
        }
    }
}

impl AltoApp {
    fn open_pdf(&mut self, path: PathBuf) {
        self.pdf_path = Some(path.clone());
        self.current_page = 1;
        
        // Get total page count using pdfinfo
        if let Ok(output) = std::process::Command::new("pdfinfo")
            .arg(&path)
            .output()
        {
            if output.status.success() {
                let info = String::from_utf8_lossy(&output.stdout);
                for line in info.lines() {
                    if line.starts_with("Pages:") {
                        if let Some(pages_str) = line.split_whitespace().nth(1) {
                            self.total_pages = pages_str.parse().unwrap_or(1);
                            break;
                        }
                    }
                }
            }
        }
        
        // Sync text editor to the current page
        self.sync_text_editor_page();
    }
    
    fn sync_text_editor_page(&mut self) {
        // Update text editor to show the same page as PDF viewer
        if let Some(pdf_path) = &self.pdf_path {
            self.editor.load_page_from_pdf(
                pdf_path.to_str().unwrap_or(""), 
                self.current_page as u32
            );
        }
    }
    
    fn render_pdf_page(&mut self, ctx: &egui::Context, page: usize) -> Option<egui::TextureHandle> {
        // No caching - generate fresh each time for better performance debugging
        
        let pdf_path = self.pdf_path.as_ref()?;
        let temp_file = format!("/tmp/chonker_page_{}", page);
        
        // Calculate scaled size based on zoom level (much smaller for performance)
        let base_width = 400.0;  // Reduced for performance
        let scaled_width = (base_width * self.zoom_level) as u32;
        
        // Use pdftoppm to render the page
        let result = std::process::Command::new("pdftoppm")
            .args([
                "-f", &page.to_string(),
                "-l", &page.to_string(),
                "-png",
                "-scale-to-x", &scaled_width.to_string(),
                "-scale-to-y", "-1",  // Maintain aspect ratio
            ])
            .arg(pdf_path)
            .arg(&temp_file)
            .output();
            
        match result {
            Ok(output) => {
                if !output.status.success() {
                    return None;
                }
            },
            Err(e) => {
                println!("DEBUG: Failed to execute pdftoppm: {}", e);
                return None;
            }
        }
        
        // Try both zero-padded and non-zero-padded formats
        let image_path_padded = format!("{}-{:02}.png", temp_file, page);
        let image_path_simple = format!("{}-{}.png", temp_file, page);
        
        let image_path = if std::path::Path::new(&image_path_padded).exists() {
            image_path_padded
        } else {
            image_path_simple
        };
        
        if let Ok(img) = image::open(&image_path) {
            let mut rgba_img = img.to_rgba8();
            
            // Convert to grayscale and invert for dark mode
            for pixel in rgba_img.pixels_mut() {
                // Fast grayscale conversion using integer math
                let gray = ((pixel[0] as u16 * 77 + pixel[1] as u16 * 151 + pixel[2] as u16 * 28) >> 8) as u8;
                
                // Invert grayscale for dark mode (white text on dark background)
                let inverted_gray = 255 - gray;
                
                // Set all channels to the inverted grayscale value
                pixel[0] = inverted_gray;
                pixel[1] = inverted_gray;
                pixel[2] = inverted_gray;
                // pixel[3] stays the same (Alpha)
            }
            
            let size = [rgba_img.width() as usize, rgba_img.height() as usize];
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba_img);
            let texture = ctx.load_texture(format!("page_{}_grayscale_{}", page, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()), color_image, egui::TextureOptions::NEAREST);
            
            // Clean up temp file immediately
            let _ = std::fs::remove_file(&image_path);
            
            return Some(texture);
        } else {
            println!("DEBUG: Failed to open image file: {}", image_path);
        }
        
        None
    }
    
    fn show_pdf_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let current_page = self.current_page;
        if let Some(texture) = self.render_pdf_page(ctx, current_page) {
            let image_size = texture.size_vec2();
            
            // Use full image size without scaling down
            let display_size = image_size;
            
            // Create scrollable area for panning
            let scroll_area = egui::ScrollArea::both()
                .auto_shrink([false; 2])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
                
            let scroll_response = scroll_area.show(ui, |ui| {
                ui.add_sized(
                    display_size,
                    egui::Image::new(&texture)
                        .bg_fill(egui::Color32::BLACK)
                        .sense(egui::Sense::drag())
                )
            });
            
            // Handle drag for panning - update scroll offset
            if scroll_response.inner.dragged() {
                // egui ScrollArea handles dragging internally
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(120, 120, 120),
                    "PDF rendering failed or no PDF loaded"
                );
            });
        }
    }
}

impl eframe::App for AltoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts
        ctx.input(|i| {
            // Cmd+O to open PDF (Cmd on macOS, Ctrl on other platforms)
            if i.modifiers.command && i.key_pressed(egui::Key::O) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PDF", &["pdf"])
                    .pick_file()
                {
                    self.open_pdf(path);
                }
            }
            
            // Cmd+/Cmd- for zoom
            if i.modifiers.command {
                if i.key_pressed(egui::Key::Equals) || i.key_pressed(egui::Key::Plus) {
                    // Zoom in (faster steps)
                    self.zoom_level = (self.zoom_level * 1.5).min(5.0);
                } else if i.key_pressed(egui::Key::Minus) {
                    // Zoom out (faster steps)
                    self.zoom_level = (self.zoom_level / 1.5).max(0.3);
                } else if i.key_pressed(egui::Key::Num0) {
                    // Reset zoom
                    self.zoom_level = 2.0;
                }
            }
            
            // Arrow keys for navigation
            if self.pdf_path.is_some() {
                if i.key_pressed(egui::Key::ArrowLeft) && self.current_page > 1 {
                    self.current_page -= 1;
                    self.sync_text_editor_page();
                } else if i.key_pressed(egui::Key::ArrowRight) && self.current_page < self.total_pages {
                    self.current_page += 1;
                    self.sync_text_editor_page();
                }
            }
        });

        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(egui::Color32::from_rgb(200, 200, 200));
        ctx.set_visuals(visuals);
        
        // Top panel for controls
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::from_rgb(200, 200, 200), "CHONKER 9.5");
                ui.separator();
                
                // File open button
                if ui.button("📁 Open PDF (Cmd+O)").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PDF", &["pdf"])
                        .pick_file()
                    {
                        self.open_pdf(path);
                    }
                }
                
                ui.separator();
                
                // Show current PDF info if loaded
                if let Some(path) = &self.pdf_path {
                    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                        ui.colored_label(egui::Color32::from_rgb(180, 180, 180), filename);
                    }
                    
                    ui.separator();
                    
                    // Unified navigation (controls both PDF and text)
                    if ui.add_enabled(self.current_page > 1, egui::Button::new("◀")).clicked() {
                        self.current_page -= 1;
                        self.sync_text_editor_page();
                    }
                    
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 200, 200),
                        format!("Page {}/{}", self.current_page, self.total_pages)
                    );
                    
                    if ui.add_enabled(self.current_page < self.total_pages, egui::Button::new("▶")).clicked() {
                        self.current_page += 1;
                        self.sync_text_editor_page();
                    }
                    
                    ui.separator();
                    
                    // Zoom indicator
                    ui.colored_label(
                        egui::Color32::from_rgb(160, 160, 160),
                        format!("{}%", (self.zoom_level * 100.0) as i32)
                    );
                } else {
                    ui.colored_label(egui::Color32::from_rgb(120, 120, 120), "No PDF loaded");
                }
            });
        });
        
        // Left side panel for PDF display
        egui::SidePanel::left("pdf_panel")
            .min_width(600.0)
            .default_width(800.0)
            .frame(egui::Frame::default().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                if self.pdf_path.is_some() {
                    self.show_pdf_page(ui, ctx);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.0);
                            ui.colored_label(
                                egui::Color32::from_rgb(140, 140, 140),
                                "Click 'Open PDF' to load a document"
                            );
                            ui.add_space(20.0);
                            ui.colored_label(
                                egui::Color32::from_rgb(120, 120, 120),
                                "PDF will appear here when loaded"
                            );
                        });
                    });
                }
            });
        
        // Central panel for text editor
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                // Apply zoom to text size
                let mut style = ui.style().as_ref().clone();
                let base_font_size = 12.0;
                let zoomed_font_size = base_font_size * self.zoom_level;
                
                style.text_styles.insert(
                    egui::TextStyle::Monospace,
                    egui::FontId::new(zoomed_font_size, egui::FontFamily::Monospace)
                );
                ui.set_style(std::sync::Arc::new(style));
                
                self.editor.show(ui);
            });
    }
}

fn main() -> Result<()> {
    let app = AltoApp::default();
    
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 800.0])
            .with_title("WYSIWYG ALTO Spatial Editor - Unified Architecture"),
        ..Default::default()
    };
    
    if let Err(e) = eframe::run_native(
        "WYSIWYG ALTO",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    ) {
        eprintln!("❌ Failed: {}", e);
    }
    
    Ok(())
}