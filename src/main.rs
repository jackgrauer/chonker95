use anyhow::Result;
use eframe;
use egui;

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
        self.grid.iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
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
        let elements = Self::load_alto_page(1);
        let mut editor = Self { 
            terminal_grid: TerminalGrid::new(), 
            grid_text: String::new(), 
            state: EditorState::default() 
        };
        editor.rebuild_grid(elements);
        editor
    }
    
    fn rebuild_grid(&mut self, elements: Vec<UnifiedAltoElement>) {
        // Calculate required grid dimensions based on content
        let mut max_col = 0;
        let mut max_row = 0;
        
        // First pass: determine required size
        let mut lines = std::collections::BTreeMap::new();
        for element in &elements {
            let line_key = (element.vpos / 12.0) as i32;
            lines.entry(line_key).or_insert_with(Vec::new).push(element);
        }
        
        for (_line_key, line_elements) in lines.iter() {
            let mut sorted_elements = line_elements.clone();
            sorted_elements.sort_by(|a, b| a.hpos.partial_cmp(&b.hpos).unwrap());
            
            if let Some(first_element) = sorted_elements.first() {
                let temp_grid = TerminalGrid::new(); // for coordinate calculation
                let (start_col, row) = temp_grid.alto_to_grid(first_element.hpos, first_element.vpos);
                let mut current_col = start_col;
                
                for (word_idx, element) in sorted_elements.iter().enumerate() {
                    if word_idx > 0 {
                        current_col += 1; // space
                    }
                    current_col += element.content.chars().count();
                }
                
                max_col = max_col.max(current_col);
                max_row = max_row.max(row + 5); // Add some padding
            }
        }
        
        // Create appropriately sized grid
        let grid_width = (max_col + 20).max(140); // Minimum 140, plus padding
        let grid_height = (max_row + 10).max(60);  // Minimum 60, plus padding
        
        self.terminal_grid = TerminalGrid::new_with_size(grid_width, grid_height);
        
        // Group elements by line (similar VPOS)
        let mut lines = std::collections::BTreeMap::new();
        for element in &elements {
            let line_key = (element.vpos / 12.0) as i32;
            lines.entry(line_key).or_insert_with(Vec::new).push(element);
        }
        
        // Place each line at ALTO Y position, then flow text normally on that line
        for (_line_key, mut line_elements) in lines {
            line_elements.sort_by(|a, b| a.hpos.partial_cmp(&b.hpos).unwrap());
            
            if let Some(first_element) = line_elements.first() {
                let (start_col, mut row) = self.terminal_grid.alto_to_grid(first_element.hpos, first_element.vpos);
                let mut current_col = start_col;
                
                // Flow text naturally on this line
                for (word_idx, element) in line_elements.iter().enumerate() {
                    // Add space between words
                    if word_idx > 0 && current_col < self.terminal_grid.grid_width {
                        if row < self.terminal_grid.grid_height {
                            self.terminal_grid.grid[row][current_col] = ' ';
                            current_col += 1;
                        }
                    }
                    
                    // Check if entire word fits, if not wrap the whole word
                    let word_len = element.content.chars().count();
                    if current_col + word_len > self.terminal_grid.grid_width {
                        // Wrap entire word to next line
                        row += 1;
                        current_col = start_col;
                        if row >= self.terminal_grid.grid_height {
                            break;
                        }
                    }
                    
                    // Place word characters
                    for ch in element.content.chars() {
                        if row < self.terminal_grid.grid_height && current_col < self.terminal_grid.grid_width {
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
        let elements = Self::load_alto_page(page);
        self.rebuild_grid(elements);
    }
    
    fn load_alto_page(page: u32) -> Vec<UnifiedAltoElement> {
        // Extract all ALTO elements from PDF using our proven parsing
        
        std::process::Command::new("pdfalto")
            .args(["-f", &page.to_string(), "-l", &page.to_string(), "-readingOrder", "-noImage", "-noLineNumbers",
                   "/Users/jack/Documents/chonker_test.pdf", "/dev/stdout"])
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
                
                let response = ui.add_sized(
                    egui::vec2(content_width, content_height),
                    egui::TextEdit::multiline(&mut self.grid_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
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
}

impl Default for AltoApp {
    fn default() -> Self {
        let editor = UnifiedAltoEditor::new();
        Self { editor }
    }
}

impl eframe::App for AltoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(egui::Color32::from_rgb(0, 255, 0));
        ctx.set_visuals(visuals);
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(255, 255, 0), "CHONKER 9.5");
                    ui.separator();
                    if ui.button("◀").clicked() && self.editor.state.current_page > 1 {
                        self.editor.load_page(self.editor.state.current_page - 1);
                    }
                    ui.colored_label(egui::Color32::from_rgb(255, 255, 0), 
                                   format!("{}/{}", self.editor.state.current_page, self.editor.state.total_pages));
                    if ui.button("▶").clicked() && self.editor.state.current_page < self.editor.state.total_pages {
                        self.editor.load_page(self.editor.state.current_page + 1);
                    }
                });
                
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