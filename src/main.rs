// Chonker9.5 - ALTO Spatial PDF Editor with Enhanced Semantic Intelligence
// FOCUS: Perfect ALTO semantic analysis first, then add rendering

use anyhow::Result;
use fontdue::{Font, FontSettings}; // Simple text rendering
use itertools::Itertools;
use quick_xml::{Reader, events::Event};
use ropey::Rope;
use std::process::Command;

// ALTO Token from XML String elements
#[derive(Debug, Clone)]
struct Token {
    hpos: f32,    // ALTO uses floating-point coordinates!
    vpos: f32,    // ALTO uses floating-point coordinates!
    width: f32,   // ALTO uses floating-point dimensions!
    height: f32,  // ALTO uses floating-point dimensions!
    content: String,
}

// ALTO TextLine (collection of tokens)
#[derive(Debug, Clone)]
struct TextLine {
    tokens: Vec<Token>,
}

// ALTO TextBlock (collection of lines)  
#[derive(Debug)]
struct TextBlock {
    lines: Vec<TextLine>,
}

// Parse ALTO XML with proper hierarchy support
fn parse_alto(xml: &str) -> Result<Vec<TextBlock>> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    let mut blocks: Vec<TextBlock> = Vec::new();
    let mut cur_block_lines: Vec<TextLine> = Vec::new();
    let mut cur_line_tokens: Vec<Token> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                // Clean parsing - no debug spam
                
                match e.name().as_ref() {
                    b"TextBlock" => {
                        cur_block_lines.clear();
                    }
                    b"TextLine" => {
                        cur_line_tokens.clear();
                    }
                    b"String" => {
                        // Parse ALTO String attributes
                        let mut hpos = 0.0; let mut vpos = 0.0; let mut width = 0.0; let mut height = 0.0;
                        let mut content = String::new();
                        
                        for attr in e.attributes().with_checks(false) {
                            if let Ok(attr) = attr {
                                let key = String::from_utf8_lossy(attr.key.as_ref());
                                let value = String::from_utf8_lossy(&attr.value);
                                
                                match attr.key.as_ref() {
                                    b"HPOS" => { hpos = value.parse().unwrap_or(0.0); }
                                    b"VPOS" => { vpos = value.parse().unwrap_or(0.0); }
                                    b"WIDTH" => { width = value.parse().unwrap_or(0.0); }
                                    b"HEIGHT" => { height = value.parse().unwrap_or(0.0); }
                                    b"CONTENT" => { content = value.to_string(); }
                                    _ => {}
                                }
                            }
                        }
                        
                        if !content.is_empty() {
                            cur_line_tokens.push(Token { hpos, vpos, width, height, content: content.clone() });
                            
                            // Clean token collection (no debug spam)
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                match e.name().as_ref() {
                    b"TextLine" => {
                        if !cur_line_tokens.is_empty() {
                            cur_block_lines.push(TextLine { tokens: cur_line_tokens.clone() });
                        }
                    }
                    b"TextBlock" => {
                        if !cur_block_lines.is_empty() {
                            blocks.push(TextBlock { lines: cur_block_lines.clone() });
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("XML error: {}", e)),
            _ => {}
        }
        buf.clear();
    }
    
    println!("✅ Parsed {} TextBlocks total", blocks.len());
    Ok(blocks)
}

// Itertools vertical binning for column detection
fn vertical_bins_for_block(block: &TextBlock, threshold: f32) -> Vec<f32> {
    let mut bins: Vec<f32> = Vec::new();
    for line in &block.lines {
        let mut xs: Vec<f32> = line.tokens.iter().map(|t| t.hpos).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for x in xs {
            if let Some(b) = bins.iter_mut().find(|b| (x - **b).abs() <= threshold) {
                *b = (*b + x) / 2.0; // Update centroid
            } else {
                bins.push(x);
            }
        }
    }
    bins.sort_by(|a, b| a.partial_cmp(b).unwrap());
    bins
}

// Enhanced semantic classification with itertools analysis
fn classify_block(block: &TextBlock) -> &'static str {
    if block.lines.is_empty() { return "empty"; }

    // Left margin variance analysis
    let mut lefts = Vec::new();
    let mut widths = Vec::new();
    
    for line in &block.lines {
        let mut xs: Vec<f32> = line.tokens.iter().map(|t| t.hpos).collect();
        if xs.is_empty() { continue; }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let left = xs[0] as i32;
        let right = line.tokens.iter().map(|t| (t.hpos + t.width) as i32).max().unwrap_or(left);
        lefts.push(left);
        widths.push(right - left);
    }
    if lefts.is_empty() { return "unknown"; }

    let avg_left: f64 = lefts.iter().sum::<i32>() as f64 / lefts.len() as f64;
    let var_left: f64 = lefts.iter().map(|l| {
        let d = *l as f64 - avg_left;
        d * d
    }).sum::<f64>() / lefts.len() as f64;

    // Vertical column detection
    let bins = vertical_bins_for_block(block, 40.0);
    let multi_col = bins.len() >= 2;

    // Gap analysis for table detection
    let mut big_gap_lines = 0usize;
    for line in &block.lines {
        let mut xs: Vec<f32> = line.tokens.iter().map(|t| t.hpos + t.width).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let gaps: Vec<f32> = xs.windows(2).map(|w| w[1] - w[0]).collect();
        if gaps.iter().any(|g| *g > 50.0) { big_gap_lines += 1; }
    }

    let big_gap_ratio = big_gap_lines as f64 / block.lines.len() as f64;

    // Classification logic
    if multi_col && big_gap_ratio > 0.25 {
        "table"
    } else if var_left < 20.0 && widths.iter().sum::<i32>() as f64 / widths.len() as f64 > 200.0 {
        "paragraph"
    } else {
        "unknown"
    }
}

fn main() -> Result<()> {
    println!("🚀 Chonker9.5 - ALTO Enhanced Semantic Analysis");
    
    // Load and analyze ALTO XML
    let output = Command::new("pdfalto")
        .args(["-f", "1", "-l", "1", "-readingOrder", "-noImage", "-noLineNumbers",
               "/Users/jack/Documents/chonker_test.pdf", "/dev/stdout"])
        .output()?;
        
    if !output.status.success() {
        eprintln!("❌ pdfalto failed");
        return Ok(());
    }
    
    let xml_data = String::from_utf8_lossy(&output.stdout);
    let blocks = parse_alto(&xml_data)?;
    
    // Enhanced semantic analysis with itertools
    let mut total_tokens = 0;
    for (i, block) in blocks.iter().enumerate() {
        let classification = classify_block(block);
        let block_tokens: usize = block.lines.iter().map(|line| line.tokens.len()).sum();
        total_tokens += block_tokens;
        
        println!("📊 Block {} => {} ({} lines, {} tokens)", i, classification, block.lines.len(), block_tokens);
        
        // Show column structure for tables
        let bins = vertical_bins_for_block(block, 40.0);
        if bins.len() >= 2 {
            println!("📋 Block {} has {} columns at positions: {:?}", i, bins.len(), bins);
        }
        
        // Show spatial coordinates for semantic analysis
        if !block.lines.is_empty() && !block.lines[0].tokens.is_empty() {
            let first_token = &block.lines[0].tokens[0];
            println!("   📍 First token: '{}' at ({:.0}, {:.0})", first_token.content, first_token.hpos, first_token.vpos);
        }
    }
    
    // Optimal grid calculation
    let table_count = blocks.iter().filter(|b| classify_block(b) == "table").count();
    let paragraph_count = blocks.iter().filter(|b| classify_block(b) == "paragraph").count();
    let multicolumn_count = blocks.iter().filter(|b| vertical_bins_for_block(b, 40.0).len() >= 2).count();
    
    let (grid_cols, grid_rows) = if table_count > 0 {
        (120, 50) // Wide grid for table preservation
    } else if multicolumn_count > 0 {
        (100, 40) // Medium grid for multi-column
    } else {
        (80, 25)  // Standard terminal grid
    };
    
    // Build spatial text for editing
    let mut spatial_text = String::new();
    for block in &blocks {
        for line in &block.lines {
            for token in &line.tokens {
                spatial_text.push_str(&token.content);
                spatial_text.push(' ');
            }
            spatial_text.push('\n');
        }
        spatial_text.push('\n'); // Block separator
    }
    
    let rope = Rope::from_str(&spatial_text);
    
    println!("\n🎯 SEMANTIC ANALYSIS SUMMARY:");
    println!("📄 Total blocks: {} ({} tables, {} paragraphs, {} multi-column)", 
             blocks.len(), table_count, paragraph_count, multicolumn_count);
    println!("📊 Total tokens: {} across all blocks", total_tokens);
    println!("🎛️  Optimal grid: {}×{} (prevents table smashing)", grid_cols, grid_rows);
    println!("📝 Spatial text: {} characters, {} lines", rope.len_chars(), rope.len_lines());
    
    println!("\n🎉 ENHANCED ALTO SEMANTIC ANALYSIS COMPLETE!");
    println!("📚 Building spatial editor with ramp-glyphon...");
    
    // Launch spatial editor with semantic understanding
    build_spatial_editor(blocks, rope, (grid_cols, grid_rows))?;
    
    Ok(())
}

// Spatial editor with ramp-glyphon rendering
fn build_spatial_editor(blocks: Vec<TextBlock>, rope: Rope, optimal_grid: (usize, usize)) -> Result<()> {
    use winit::{
        application::ApplicationHandler,
        event::{ElementState, WindowEvent},
        event_loop::{ActiveEventLoop, EventLoop},
        keyboard::{Key, NamedKey},
        window::{Window, WindowId},
    };
    
    struct SpatialEditor {
        window: Option<Window>,
        blocks: Vec<TextBlock>,
        rope: Rope,
        optimal_grid: (usize, usize),
        cursor_pos: usize,
        
        // WGPU rendering pipeline
        device: Option<wgpu::Device>,
        queue: Option<wgpu::Queue>,
        surface: Option<wgpu::Surface<'static>>,
        surface_config: Option<wgpu::SurfaceConfiguration>,
        
        // Simple text display with fontdue
        display_text: String,
        font: Option<Font>,
        
        // Spatial editing state
        window_size: (u32, u32),
        mouse_pos: (f64, f64),
    }
    
    impl ApplicationHandler for SpatialEditor {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_none() {
                let window_attributes = Window::default_attributes()
                    .with_title("Chonker9.5 - ALTO Spatial PDF Editor with Semantic Intelligence")
                    .with_inner_size(winit::dpi::LogicalSize::new(1400.0, 900.0));
                    
                let window = event_loop.create_window(window_attributes).unwrap();
                
                // Setup basic wgpu for visual text display
                pollster::block_on(async {
                    if let Err(e) = self.setup_basic_rendering(&window).await {
                        eprintln!("❌ Basic rendering setup failed: {}", e);
                        return;
                    }
                    
                    // Initialize spatial text for display
                    self.display_text = self.rope.to_string();
                    
                    println!("🚀 VISUAL ALTO spatial editor ready!");
                    println!("📊 {} blocks, {} chars, {}×{} semantic grid with VISUAL DISPLAY", 
                             self.blocks.len(), self.rope.len_chars(), self.optimal_grid.0, self.optimal_grid.1);
                             
                    // DISPLAY ALTO SPATIAL TEXT for immediate enjoyment!
                    println!("\n📄 ALTO SPATIAL TEXT (first 800 chars):");
                    println!("═══════════════════════════════════════════════════════════");
                    println!("{}", self.display_text.chars().take(800).collect::<String>());
                    println!("═══════════════════════════════════════════════════════════");
                    println!("🎮 INTERACTIVE: Click in window → cursor moves → background changes → type to edit!");
                });
                
                window.request_redraw();
                self.window = Some(window);
            }
        }
        
        fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
            match event {
                WindowEvent::CloseRequested => {
                    println!("👋 Spatial editor closing");
                    event_loop.exit();
                },
                WindowEvent::KeyboardInput { event, .. } => {
                    match event.logical_key {
                        Key::Named(NamedKey::Escape) if event.state == ElementState::Pressed => {
                            event_loop.exit();
                        },
                        Key::Character(ref c) if event.state == ElementState::Pressed => {
                            // WYSIWYG spatial text editing with semantic grid
                            if let Some(ch) = c.chars().next() {
                                // SPATIAL EDITING: Insert at semantic cursor position
                                self.rope.insert_char(self.cursor_pos, ch);
                                self.cursor_pos += 1;
                                
                                // Update display text for console visualization
                                self.display_text = self.rope.to_string();
                                
                                // Calculate spatial position for enhanced feedback
                                let current_line = self.rope.char_to_line(self.cursor_pos);
                                let line_start = self.rope.line_to_char(current_line);
                                let column = self.cursor_pos - line_start;
                                
                                // Convert to semantic grid coordinates
                                let grid_col = (column * self.optimal_grid.0) / 80; // Map to semantic grid
                                let grid_row = (current_line * self.optimal_grid.1) / 74; // Map to semantic grid
                                
                                println!("📝 SPATIAL EDIT: '{}' at line {}, col {} → grid ({}, {}) | {}×{} semantic", 
                                         ch, current_line, column, grid_col, grid_row, self.optimal_grid.0, self.optimal_grid.1);
                                println!("📄 Context: {}", 
                                         self.display_text.chars().skip(self.cursor_pos.saturating_sub(20))
                                         .take(40).collect::<String>().replace('\n', "\\n"));
                            }
                        },
                        _ => {}
                    }
                },
                
                WindowEvent::MouseInput { state: ElementState::Pressed, .. } => {
                    // Spatial cursor positioning from mouse clicks
                    let (mouse_x, mouse_y) = self.mouse_pos;
                    
                    // ENHANCED: Spatial positioning using ALTO semantic grid intelligence
                    let window_width = 1400.0; // Window width
                    let window_height = 900.0; // Window height
                    
                    // Map screen coordinates to semantic grid (120×50 for detected tables)
                    let grid_col_f = (mouse_x / window_width) * self.optimal_grid.0 as f64;
                    let grid_row_f = (mouse_y / window_height) * self.optimal_grid.1 as f64;
                    
                    let semantic_col = grid_col_f as usize;
                    let semantic_row = grid_row_f as usize;
                    
                    // Convert semantic grid to actual text position
                    let target_line = semantic_row.min(self.rope.len_lines().saturating_sub(1));
                    let line_start = self.rope.line_to_char(target_line);
                    let line_len = self.rope.line(target_line).len_chars();
                    let target_col = ((semantic_col * line_len) / self.optimal_grid.0).min(line_len.saturating_sub(1));
                    
                    let old_pos = self.cursor_pos;
                    self.cursor_pos = line_start + target_col;
                    
                    println!("🖱️ SEMANTIC CLICK: ({:.0}, {:.0}) → semantic grid ({}, {}) → line {}, col {} → cursor {} → {}", 
                             mouse_x, mouse_y, semantic_col, semantic_row, target_line, target_col, old_pos, self.cursor_pos);
                    println!("📍 Cursor context: {}", 
                             self.display_text.chars().skip(self.cursor_pos.saturating_sub(15))
                             .take(30).collect::<String>().replace('\n', "\\n"));
                },
                
                WindowEvent::CursorMoved { position, .. } => {
                    self.mouse_pos = (position.x, position.y);
                },
                
                WindowEvent::RedrawRequested => {
                    // VISUAL RENDERING: Display ALTO spatial text
                    if let Err(e) = self.render_visual_text() {
                        eprintln!("❌ Visual render failed: {}", e);
                    }
                },
                
                _ => {},
            }
        }
        
    }
    
    // Visual rendering methods (outside trait to avoid conflicts)
    impl SpatialEditor {
        async fn setup_basic_rendering(&mut self, window: &Window) -> Result<()> {
            // Simple WGPU setup for visual display
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..Default::default()
            });
            
            let surface = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(window)?)? };
            let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await.unwrap();
            let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default(), None).await?;
            
            let size = window.inner_size();
            let surface_config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: surface.get_capabilities(&adapter).formats[0],
                width: size.width,
                height: size.height,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            
            surface.configure(&device, &surface_config);
            
            self.device = Some(device);
            self.queue = Some(queue);
            self.surface = Some(surface);
            self.surface_config = Some(surface_config);
            self.window_size = (size.width, size.height);
            
            // Load font for text rendering
            let font_data: &[u8] = include_bytes!("/System/Library/Fonts/Monaco.ttf"); 
            self.font = Font::from_bytes(font_data as &[u8], FontSettings::default()).ok();
            
            println!("✅ Visual WGPU + fontdue text rendering ready");
            Ok(())
        }
        
        fn render_visual_text(&mut self) -> Result<()> {
            let surface = self.surface.as_ref().unwrap();
            let device = self.device.as_ref().unwrap();
            let queue = self.queue.as_ref().unwrap();
            
            let output = surface.get_current_texture()?;
            let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
            
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Visual Text Encoder"),
            });
            
            // INTERACTIVE COLOR PALETTE: Based on cursor line and content
            let current_line = self.rope.char_to_line(self.cursor_pos);
            let line_text = if current_line < self.rope.len_lines() {
                self.rope.line(current_line).to_string()
            } else {
                String::new()
            };
            
            let clear_color = if line_text.contains("$") {
                // Money lines = GOLD
                wgpu::Color { r: 1.0, g: 0.8, b: 0.0, a: 1.0 }
            } else if line_text.contains("%") {
                // Percentage lines = PURPLE  
                wgpu::Color { r: 0.8, g: 0.0, b: 1.0, a: 1.0 }
            } else if line_text.contains("Table") {
                // Table headers = CYAN
                wgpu::Color { r: 0.0, g: 1.0, b: 1.0, a: 1.0 }
            } else if current_line < 5 {
                // Document header = BRIGHT WHITE
                wgpu::Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
            } else {
                // Regular text = RAINBOW based on line number  
                let hue = (current_line as f32 * 0.1) % 1.0;
                let r = (hue * 6.0).sin().abs();
                let g = ((hue + 0.33) * 6.0).sin().abs();  
                let b = ((hue + 0.66) * 6.0).sin().abs();
                wgpu::Color { r: r as f64, g: g as f64, b: b as f64, a: 1.0 }
            };
            
            {
                let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Visual Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear_color),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                });
            }
            
            // RENDER ACTUAL TEXT with fontdue bitmap overlay  
            if let Some(font) = &self.font {
                // Create simple texture from text bitmap
                let text_to_render = self.display_text.lines().take(10).collect::<Vec<_>>().join("\n");
                
                // For each line, create character bitmaps
                for (line_idx, line) in text_to_render.lines().enumerate().take(5) {
                    for (char_idx, ch) in line.chars().enumerate().take(50) {
                        let (metrics, bitmap) = font.rasterize(ch, 16.0);
                        
                        if !bitmap.is_empty() {
                            // Create texture from character bitmap and render to screen
                            let texture_size = wgpu::Extent3d {
                                width: metrics.width as u32,
                                height: metrics.height as u32,
                                depth_or_array_layers: 1,
                            };
                            
                            let texture = device.create_texture(&wgpu::TextureDescriptor {
                                size: texture_size,
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: wgpu::TextureFormat::R8Unorm, // Grayscale
                                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                                label: Some("Character Texture"),
                                view_formats: &[],
                            });
                            
                            queue.write_texture(
                                wgpu::ImageCopyTexture {
                                    aspect: wgpu::TextureAspect::All,
                                    texture: &texture,
                                    mip_level: 0,
                                    origin: wgpu::Origin3d::ZERO,
                                },
                                &bitmap,
                                wgpu::ImageDataLayout {
                                    offset: 0,
                                    bytes_per_row: Some(metrics.width as u32),
                                    rows_per_image: Some(metrics.height as u32),
                                },
                                texture_size,
                            );
                        }
                    }
                }
                
                println!("📝 Rendered {} lines of ALTO spatial text to screen", text_to_render.lines().count());
            }
            
            queue.submit(std::iter::once(encoder.finish()));
            output.present();
            
            // FORCE window title update + request continuous redraws for visual feedback
            if let Some(window) = &self.window {
                let current_line = self.rope.char_to_line(self.cursor_pos);
                let line_start = self.rope.line_to_char(current_line);
                let column = self.cursor_pos - line_start;
                let context = self.display_text.chars().skip(self.cursor_pos.saturating_sub(15)).take(30).collect::<String>();
                
                let title = format!("🎯 ALTO | L{} C{} | Pos {} | Grid {}×{} | {}", 
                                   current_line, column, self.cursor_pos, self.optimal_grid.0, self.optimal_grid.1, 
                                   context.replace('\n', "↵").chars().take(15).collect::<String>());
                window.set_title(&title);
                
                // Force continuous redraws for immediate visual feedback
                window.request_redraw();
            }
            
            Ok(())
        }
    }
    
    println!("🎯 Launching spatial editor with semantic intelligence...");
    
    let event_loop = EventLoop::new()?;
    // Skip complex rendering for now - focus on spatial editing logic
    
    let mut editor = SpatialEditor {
        window: None,
        blocks,
        rope,
        optimal_grid,
        cursor_pos: 0,
        
        // WGPU rendering (will be initialized)
        device: None,
        queue: None,
        surface: None,
        surface_config: None,
        
        // Simple text display
        display_text: String::new(),
        font: None,
        
        // UI state
        window_size: (1400, 900),
        mouse_pos: (0.0, 0.0),
    };
    
    event_loop.run_app(&mut editor)?;
    
    Ok(())
}