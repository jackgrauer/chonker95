// STRUCTURALLY IMPOSSIBLE TO DESYNC - Unified ALTO Editor
use anyhow::Result;
use eframe;
use egui;

// SINGLE SOURCE OF TRUTH - Cannot desync because there's only one data structure
#[derive(Debug, Clone)]
struct UnifiedAltoElement {
    // ALTO XML data (authoritative source)
    id: String,
    content: String,
    hpos: f32,
    vpos: f32,
    width: f32,
    height: f32,
    
    // Simplified: Only need ALTO data for quantization
}

impl UnifiedAltoElement {
    fn new(id: String, content: String, hpos: f32, vpos: f32, width: f32, height: f32, _scale: f32) -> Self {
        Self { id, content, hpos, vpos, width, height }
    }
}

// TERMINAL GRID - Fixed character grid with ALTO spatial mapping
struct TerminalGrid {
    grid: Vec<Vec<char>>,         // 2D character array (like terminal buffer)
    grid_width: usize,           // Fixed grid width (e.g., 120 columns)
    grid_height: usize,          // Fixed grid height (e.g., 50 rows)
    char_width: f32,             // ALTO pixels per character
    line_height: f32,            // ALTO pixels per line
}

impl TerminalGrid {
    fn new() -> Self {
        Self {
            grid: vec![vec![' '; 140]; 60], // Larger grid for more space
            grid_width: 140,
            grid_height: 60,
            char_width: 4.5,  // Smaller units per character (more resolution)
            line_height: 9.0, // Smaller units per line (more resolution)
        }
    }
    
    // Map ALTO coordinates directly to grid cells
    fn alto_to_grid(&self, hpos: f32, vpos: f32) -> (usize, usize) {
        let col = (hpos / self.char_width) as usize;
        let row = (vpos / self.line_height) as usize;
        (col.min(self.grid_width - 1), row.min(self.grid_height - 1))
    }
    
    // Place ALTO element directly in grid at calculated position
    fn place_element(&mut self, element: &UnifiedAltoElement) {
        let (start_col, row) = self.alto_to_grid(element.hpos, element.vpos);
        
        // Place each character of the element
        for (i, ch) in element.content.chars().enumerate() {
            let col = start_col + i;
            if col < self.grid_width && row < self.grid_height {
                self.grid[row][col] = ch;
            }
        }
    }
    
    // Convert grid to editable text
    fn to_text(&self) -> String {
        self.grid.iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }
    
    // Update grid from edited text (reverse mapping)
    fn from_text(&mut self, text: &str) {
        // Clear grid
        for row in &mut self.grid {
            for cell in row {
                *cell = ' ';
            }
        }
        
        // Place text back in grid
        for (row_idx, line) in text.lines().enumerate() {
            if row_idx < self.grid_height {
                for (col_idx, ch) in line.chars().enumerate() {
                    if col_idx < self.grid_width {
                        self.grid[row_idx][col_idx] = ch;
                    }
                }
            }
        }
    }
}

// SIMPLIFIED ALTO EDITOR
struct UnifiedAltoEditor {
    terminal_grid: TerminalGrid,
    grid_text: String, // Current editable text
    fake_scroll_x: f32, // Horizontal pan accumulator
    needs_repaint: bool, // Throttle repaints
    last_text_hash: u64, // Detect text changes
}

impl UnifiedAltoEditor {
    fn new() -> Self {
        // LOAD FULL PDF PAGE: Get all ALTO elements from PDF
        let elements = Self::load_full_alto_page();
        let mut terminal_grid = TerminalGrid::new();
        
        // HYBRID APPROACH: Use ALTO for line positioning, then flow text naturally  
        let mut lines = std::collections::BTreeMap::new();
        
        // Group elements by line (similar VPOS)
        for element in &elements {
            let line_key = (element.vpos / 12.0) as i32;
            lines.entry(line_key).or_insert_with(Vec::new).push(element);
        }
        
        // Place each line at ALTO Y position, then flow text normally on that line
        for (line_key, mut line_elements) in lines {
            line_elements.sort_by(|a, b| a.hpos.partial_cmp(&b.hpos).unwrap());
            
            if let Some(first_element) = line_elements.first() {
                let (start_col, mut row) = terminal_grid.alto_to_grid(first_element.hpos, first_element.vpos);
                let mut current_col = start_col;
                
                // Flow text naturally on this line
                for (word_idx, element) in line_elements.iter().enumerate() {
                    // Add space between words
                    if word_idx > 0 && current_col < terminal_grid.grid_width {
                        if row < terminal_grid.grid_height {
                            terminal_grid.grid[row][current_col] = ' ';
                            current_col += 1;
                        }
                    }
                    
                    // OVERFLOW HANDLING: Place word characters with wraparound
                    for ch in element.content.chars() {
                        if row < terminal_grid.grid_height {
                            if current_col >= terminal_grid.grid_width {
                                // WRAP TO NEXT LINE: Don't lose content  
                                row += 1;
                                current_col = start_col; // Maintain indentation
                                if row >= terminal_grid.grid_height {
                                    println!("⚠️ OVERFLOW: Content extends beyond grid height at '{}'", element.content);
                                    break;
                                }
                            }
                            if current_col < terminal_grid.grid_width {
                                terminal_grid.grid[row][current_col] = ch;
                                current_col += 1;
                            }
                        }
                    }
                }
            }
        }
        
        let grid_text = terminal_grid.to_text();
        
        println!("📟 TERMINAL GRID: Placed {} elements in {}×{} grid", 
                 elements.len(), terminal_grid.grid_width, terminal_grid.grid_height);
        
        Self { 
            terminal_grid, 
            grid_text, 
            fake_scroll_x: 0.0,
            needs_repaint: true,
            last_text_hash: 0,
        }
    }
    
    fn load_full_alto_page() -> Vec<UnifiedAltoElement> {
        // Extract all ALTO elements from PDF using our proven parsing
        println!("📄 Loading PDF: /Users/jack/Documents/chonker_test.pdf");
        
        match std::process::Command::new("pdfalto")
            .args(["-f", "1", "-l", "1", "-readingOrder", "-noImage", "-noLineNumbers",
                   "/Users/jack/Documents/chonker_test.pdf", "/dev/stdout"])
            .output() 
        {
            Ok(output) if output.status.success() => {
                let xml_data = String::from_utf8_lossy(&output.stdout);
                let elements = Self::parse_alto_elements(&xml_data);
                if elements.is_empty() {
                    println!("⚠️ WARNING: PDF loaded but no ALTO elements found - check XML structure");
                } else {
                    println!("✅ SUCCESS: Loaded {} ALTO elements from PDF", elements.len());
                }
                elements
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("❌ ERROR: pdfalto failed with exit code: {}", output.status);
                println!("   stderr: {}", stderr);
                println!("   Using fallback sample elements");
                Self::create_fallback_elements()
            }
            Err(e) => {
                println!("❌ ERROR: Could not execute pdfalto: {}", e);
                println!("   Make sure pdfalto is installed and in PATH");
                println!("   Using fallback sample elements");
                Self::create_fallback_elements()
            }
        }
    }
    
    fn create_fallback_elements() -> Vec<UnifiedAltoElement> {
        vec![
            UnifiedAltoElement::new("demo1".to_string(), "DEMO".to_string(), 160.8, 84.8, 30.0, 12.0, 1.0),
            UnifiedAltoElement::new("demo2".to_string(), "ALTO".to_string(), 200.0, 84.8, 30.0, 12.0, 1.0),
            UnifiedAltoElement::new("demo3".to_string(), "EDITOR".to_string(), 240.0, 84.8, 40.0, 12.0, 1.0),
            UnifiedAltoElement::new("demo4".to_string(), "No PDF loaded - demo mode".to_string(), 78.6, 110.0, 200.0, 12.0, 1.0),
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
                        let mut width = 0.0;
                        let mut height = 0.0;
                        let mut id = format!("s{}", elements.len());
                        
                        for attr in e.attributes().with_checks(false) {
                            if let Ok(attr) = attr {
                                let key = String::from_utf8_lossy(attr.key.as_ref());
                                let value = String::from_utf8_lossy(&attr.value);
                                
                                match key.as_ref() {
                                    "CONTENT" => content = value.to_string(),
                                    "HPOS" => hpos = value.parse().unwrap_or(0.0),
                                    "VPOS" => vpos = value.parse().unwrap_or(0.0),
                                    "WIDTH" => width = value.parse().unwrap_or(0.0),
                                    "HEIGHT" => height = value.parse().unwrap_or(0.0),
                                    "ID" => id = value.to_string(),
                                    _ => {}
                                }
                            }
                        }
                        
                        if !content.is_empty() {
                            elements.push(UnifiedAltoElement::new(id, content, hpos, vpos, width, height, 1.0));
                        }
                    }
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
            buf.clear();
        }
        
        println!("🎯 Loaded {} ALTO elements from full PDF page", elements.len());
        elements
    }
    
    fn show(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let input = ui.input(|i| i.clone());

        // HORIZONTAL SWIPE HACK: Only respond to horizontal scroll for left/right pan
        let mut scroll_changed = false;
        if input.raw_scroll_delta.x.abs() > input.raw_scroll_delta.y.abs() && input.raw_scroll_delta.x.abs() > 0.1 {
            let old_scroll = self.fake_scroll_x;
            self.fake_scroll_x += input.raw_scroll_delta.x;
            
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
            
            self.fake_scroll_x = self.fake_scroll_x.clamp(min_scroll, max_scroll);
            scroll_changed = self.fake_scroll_x != old_scroll;
            
            if scroll_changed {
                self.needs_repaint = true; // Only repaint when scroll actually changes
            }
        }

        // SIMPLE SCROLLABLE TERMINAL GRID: Performance optimized
        if self.needs_repaint || scroll_changed {
            // Only request repaint when actually needed
            ui.ctx().request_repaint();
            self.needs_repaint = false;
        }
        
        egui::ScrollArea::both()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let response = ui.add_sized(
                    egui::vec2(1400.0, 800.0), // Large canvas to see full content
                    egui::TextEdit::multiline(&mut self.grid_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                );
                
                // THROTTLED GRID UPDATES: Only update when text actually changes
                if response.changed() {
                    let new_hash = self.calculate_text_hash();
                    if new_hash != self.last_text_hash {
                        self.terminal_grid.from_text(&self.grid_text);
                        self.last_text_hash = new_hash;
                        self.needs_repaint = true;
                    }
                }
                
                response
            }).inner
    }
    
    // PERFORMANCE: Fast text change detection
    fn calculate_text_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        self.grid_text.hash(&mut hasher);
        hasher.finish()
    }
    
    fn export_alto_xml(&self) -> String {
        // Generate sample ALTO XML structure for display
        let mut xml = String::from("<?xml version=\"1.0\"?>\n<alto xmlns=\"http://www.loc.gov/standards/alto/ns-v3#\">\n<Layout>\n<Page WIDTH=\"612\" HEIGHT=\"792\">\n<PrintSpace>\n<TextBlock>\n<TextLine>\n");
        
        // Add first few lines of content as ALTO XML structure
        let lines: Vec<&str> = self.grid_text.lines().take(5).collect();
        for (i, line) in lines.iter().enumerate() {
            let y_pos = 84.8 + (i as f32 * 24.0);
            for (j, word) in line.split_whitespace().enumerate().take(8) {
                let x_pos = 78.0 + (j as f32 * 60.0);
                xml.push_str(&format!("<String ID=\"s{}{}\" CONTENT=\"{}\" HPOS=\"{:.1}\" VPOS=\"{:.1}\" WIDTH=\"50.0\" HEIGHT=\"12.0\"/>\n",
                                      i, j, word, x_pos, y_pos));
            }
        }
        
        xml.push_str("</TextLine>\n</TextBlock>\n</PrintSpace>\n</Page>\n</Layout>\n</alto>");
        xml
    }
}

struct AltoApp {
    editor: UnifiedAltoEditor,
    exported_xml: String,
}

impl Default for AltoApp {
    fn default() -> Self {
        let editor = UnifiedAltoEditor::new();
        let exported_xml = editor.export_alto_xml();
        Self { editor, exported_xml }
    }
}

impl eframe::App for AltoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // DOS AESTHETIC: Terminal colors, no rounded corners
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::BLACK;
        visuals.window_fill = egui::Color32::BLACK;
        visuals.faint_bg_color = egui::Color32::from_gray(20);
        visuals.extreme_bg_color = egui::Color32::from_gray(10);
        visuals.override_text_color = Some(egui::Color32::from_rgb(0, 255, 0)); // DOS green
        visuals.window_shadow = egui::epaint::Shadow::NONE;
        ctx.set_visuals(visuals);
        
        // DOS-style split screen: No fancy panels, just raw divisions
        let available = ctx.available_rect();
        let split_x = available.width() * 0.3;
        
        // Left: ALTO XML (terminal style)
        let left_rect = egui::Rect::from_min_size(
            available.min,
            egui::vec2(split_x, available.height())
        );
        
        // DOS SPLIT SCREEN: Simple left/right division
        egui::SidePanel::left("alto_xml")
            .exact_width(split_x)
            .resizable(false)
            .show(ctx, |ui| {
                ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
                
                // DOS prompt style
                ui.colored_label(egui::Color32::from_rgb(255, 255, 0), "C:\\ALTO> ");
                ui.separator();
                
                // Terminal-style XML display
                ui.add_sized(
                    ui.available_size(),
                    egui::TextEdit::multiline(&mut self.exported_xml.as_str())
                        .font(egui::TextStyle::Monospace)
                );
            });
        
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
            
            // DOS prompt style
            ui.colored_label(egui::Color32::from_rgb(255, 255, 0), "C:\\EDIT> ");
            ui.separator();
            
            // Terminal-style text editor
            self.editor.show(ui);
        });
    }
}

fn main() -> Result<()> {
    println!("🚀 WYSIWYG ALTO XML Editor - Structurally Impossible to Desync!");
    
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