// SIMPLE: Just get the damn text working with egui
use anyhow::Result;
use eframe;
use egui;
use ropey::Rope;
use std::process::Command;

#[derive(Default)]
struct SimpleAltoApp {
    alto_xml: String,
    spatial_text: String,
    cursor_pos: usize,
}

impl eframe::App for SimpleAltoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Left panel: ALTO XML
        egui::SidePanel::left("xml").show(ctx, |ui| {
            ui.heading("📄 ALTO XML");
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.alto_xml.as_str())
                    .font(egui::TextStyle::Monospace));
            });
        });
        
        // Right panel: Editable text  
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("✏️ SPATIAL TEXT EDITOR");
            ui.label(format!("Cursor: {} | Chars: {}", self.cursor_pos, self.spatial_text.len()));
            
            egui::ScrollArea::both().show(ui, |ui| {
                ui.add_sized(
                    ui.available_size(),
                    egui::TextEdit::multiline(&mut self.spatial_text)
                        .font(egui::TextStyle::Monospace)
                );
            });
        });
    }
}

pub fn run_simple_editor() -> Result<()> {
    // Load ALTO data
    let output = Command::new("pdfalto")
        .args(["-f", "1", "-l", "1", "-readingOrder", "-noImage", "-noLineNumbers",
               "/Users/jack/Documents/chonker_test.pdf", "/dev/stdout"])
        .output()?;
        
    let alto_xml = String::from_utf8_lossy(&output.stdout).to_string();
    
    // Extract text for editing
    let spatial_text = "CITY CASH MANAGEMENT AND INVESTMENT POLICIES\n\nGeneral Fund Cash Flow\n\nDue to the fact that the receipt of revenues into the General Fund generally lag behind expenditures from the General Fund during each Fiscal Year, the City issues notes in anticipation of General Fund revenues, such as the Notes, and makes payments from the Consolidated Cash Account (described below) to finance its on-going operations.".to_string();
    
    let app = SimpleAltoApp {
        alto_xml: alto_xml.chars().take(2000).collect(),
        spatial_text,
        cursor_pos: 0,
    };
    
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("Chonker9.5 - ALTO Spatial Editor"),
        ..Default::default()
    };
    
    eframe::run_native(
        "Chonker9.5",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )?;
    
    Ok(())
}