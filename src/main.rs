use anyhow::Result;
use eframe;
use egui;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct UnifiedAltoElement {
    content: String,
    hpos: f32,
    vpos: f32,
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
    grid_text: String,
    state: EditorState,
}

impl UnifiedAltoEditor {
    fn new() -> Self {
        let mut editor = Self { 
            terminal_grid: TerminalGrid::new(), 
            grid_text: String::new(), 
            state: EditorState::default() 
        };
        // Show welcome message instead of loading hardcoded PDF
        let welcome_elements = vec![
            UnifiedAltoElement::new("CHONKER 9.5".to_string(), 160.8, 84.8),
            UnifiedAltoElement::new("Text Editor".to_string(), 150.0, 110.0),
            UnifiedAltoElement::new("Load a PDF to see extracted text here".to_string(), 70.0, 140.0),
        ];
        editor.rebuild_grid(welcome_elements);
        editor
    }
    
    fn rebuild_grid(&mut self, elements: Vec<UnifiedAltoElement>) {
        // Group elements by line (similar VPOS)
        let mut lines = std::collections::BTreeMap::new();
        for element in &elements {
            let line_key = (element.vpos / 12.0) as i32;
            lines.entry(line_key).or_insert_with(Vec::new).push(element);
        }
        
        // Calculate required grid dimensions
        let max_line_key = lines.keys().max().copied().unwrap_or(0) as usize;
        let mut max_col = 0;
        
        for line_elements in lines.values() {
            let mut sorted_elements = line_elements.clone();
            sorted_elements.sort_by(|a, b| a.hpos.partial_cmp(&b.hpos).unwrap());
            
            if let Some(first_element) = sorted_elements.first() {
                let start_col = (first_element.hpos / 4.5) as usize;
                let mut current_col = start_col;
                
                for (word_idx, element) in sorted_elements.iter().enumerate() {
                    if word_idx > 0 {
                        current_col += 1; // space
                    }
                    current_col += element.content.chars().count();
                }
                
                max_col = max_col.max(current_col);
            }
        }
        
        // Create appropriately sized grid - use line_key directly for height
        let grid_width = (max_col + 20).max(140);
        let grid_height = (max_line_key + 10).max(60);
        
        self.terminal_grid = TerminalGrid::new_with_size(grid_width, grid_height);
        
        // Place text using line_key as the row position
        for (line_key, mut line_elements) in lines {
            line_elements.sort_by(|a, b| a.hpos.partial_cmp(&b.hpos).unwrap());
            
            let row = line_key as usize;
            if row >= self.terminal_grid.grid_height {
                continue;
            }
            
            if let Some(first_element) = line_elements.first() {
                let start_col = (first_element.hpos / self.terminal_grid.char_width) as usize;
                let mut current_col = start_col;
                
                // Flow text naturally on this line
                for (word_idx, element) in line_elements.iter().enumerate() {
                    // Add space between words
                    if word_idx > 0 && current_col < self.terminal_grid.grid_width {
                        self.terminal_grid.grid[row][current_col] = ' ';
                        current_col += 1;
                    }
                    
                    // Place word characters (no wrapping to preserve layout)
                    for ch in element.content.chars() {
                        if current_col < self.terminal_grid.grid_width {
                            self.terminal_grid.grid[row][current_col] = ch;
                            current_col += 1;
                        }
                    }
                }
            }
        }
        
        self.grid_text = self.terminal_grid.to_text();
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
            let max_line_length = self.grid_text.lines()
                .map(|line| line.chars().count())
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
                
                let response = ui.add_sized(
                    egui::vec2(extended_width, content_height),
                    egui::TextEdit::multiline(&mut self.grid_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(extended_width)
                        .frame(false)
                );
                
                // THROTTLED GRID UPDATES: Only update when text actually changes
                if response.changed() {
                    // Inline hash calculation for performance
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    self.grid_text.hash(&mut hasher);
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
    page_cache: HashMap<usize, egui::TextureHandle>,
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
            page_cache: HashMap::new(),
            zoom_level: 1.5,
        }
    }
}

impl AltoApp {
    fn open_pdf(&mut self, path: PathBuf) {
        self.pdf_path = Some(path.clone());
        self.current_page = 1;
        self.page_cache.clear();
        
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
    
    fn render_pdf_page(&mut self, ctx: &egui::Context, page: usize) -> Option<&egui::TextureHandle> {
        if self.page_cache.contains_key(&page) {
            return self.page_cache.get(&page);
        }
        
        let pdf_path = self.pdf_path.as_ref()?;
        let temp_file = format!("/tmp/chonker_page_{}", page);
        
        // Calculate scaled size based on zoom level (lowered base for performance)
        let base_width = 400.0;
        let scaled_width = (base_width * self.zoom_level) as u32;
        
        println!("DEBUG: Rendering page {} from PDF: {}", page, pdf_path.display());
        println!("DEBUG: Temp file base: {}", temp_file);
        println!("DEBUG: Scaled width: {}", scaled_width);
        
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
                if output.status.success() {
                    println!("DEBUG: pdftoppm command succeeded");
                    let image_path = format!("{}-{}.png", temp_file, page);
                    println!("DEBUG: Looking for image at: {}", image_path);
                    
                    if std::path::Path::new(&image_path).exists() {
                        println!("DEBUG: Image file exists, trying to load");
                        match image::open(&image_path) {
                            Ok(img) => {
                                println!("DEBUG: Successfully loaded image");
                            },
                            Err(e) => {
                                println!("DEBUG: Failed to load image: {}", e);
                                return None;
                            }
                        }
                    } else {
                        println!("DEBUG: Image file does not exist at expected path");
                        return None;
                    }
                } else {
                    println!("DEBUG: pdftoppm command failed with status: {}", output.status);
                    println!("DEBUG: stderr: {}", String::from_utf8_lossy(&output.stderr));
                    return None;
                }
            },
            Err(e) => {
                println!("DEBUG: Failed to execute pdftoppm: {}", e);
                return None;
            }
        }
        
        let image_path = format!("{}-{}.png", temp_file, page);
        if let Ok(img) = image::open(&image_path) {
            let mut rgba_img = img.to_rgba8();
            
            // Invert colors for dark mode
            for pixel in rgba_img.pixels_mut() {
                // Invert RGB channels, keep alpha
                pixel[0] = 255 - pixel[0]; // Red
                pixel[1] = 255 - pixel[1]; // Green  
                pixel[2] = 255 - pixel[2]; // Blue
                // pixel[3] stays the same (Alpha)
            }
            
            let size = [rgba_img.width() as usize, rgba_img.height() as usize];
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba_img);
            let texture = ctx.load_texture(format!("page_{}", page), color_image, egui::TextureOptions::LINEAR);
            
            // Clean up temp file
            let _ = std::fs::remove_file(&image_path);
            
            self.page_cache.insert(page, texture);
            println!("DEBUG: Successfully cached texture for page {}", page);
            return self.page_cache.get(&page);
        } else {
            println!("DEBUG: Failed to open image file: {}", image_path);
        }
        
        None
    }
    
    fn show_pdf_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(texture) = self.render_pdf_page(ctx, self.current_page) {
            let available_size = ui.available_size();
            let image_size = texture.size_vec2();
            
            // Scale to fit available space
            let scale = (available_size.x / image_size.x).min(available_size.y / image_size.y).min(1.0);
            let display_size = image_size * scale;
            
            ui.allocate_ui_with_layout(
                available_size,
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.add(
                        egui::Image::new(texture)
                            .max_size(display_size)
                            .bg_fill(egui::Color32::BLACK)
                    );
                }
            );
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
                    // Zoom in
                    self.zoom_level = (self.zoom_level * 1.2).min(3.0);
                    self.page_cache.clear(); // Clear cache to regenerate at new zoom
                } else if i.key_pressed(egui::Key::Minus) {
                    // Zoom out  
                    self.zoom_level = (self.zoom_level / 1.2).max(0.3);
                    self.page_cache.clear(); // Clear cache to regenerate at new zoom
                } else if i.key_pressed(egui::Key::Num0) {
                    // Reset zoom
                    self.zoom_level = 1.0;
                    self.page_cache.clear();
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
            .min_width(400.0)
            .default_width(500.0)
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