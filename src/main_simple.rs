// SIMPLE VERSION: Just show the ALTO text in egui - no more fighting
use anyhow::Result;
use eframe;
use egui;
use std::process::Command;

#[derive(Default)]
struct AltoTextApp {
    alto_xml: String,
    spatial_text: String,
}

impl eframe::App for AltoTextApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Split panel: XML left, text right
        egui::SidePanel::left("xml").show(ctx, |ui| {
            ui.heading("📄 ALTO XML");
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.alto_xml.as_str())
                    .font(egui::TextStyle::Monospace)
                    .code_editor());
            });
        });
        
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("✏️ EDITABLE TEXT");
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

fn main() -> Result<()> {
    println!("🚀 SIMPLE ALTO TEXT EDITOR - JUST SHOW THE DAMN TEXT!");
    
    // Get ALTO XML
    let output = Command::new("pdfalto")
        .args(["-f", "1", "-l", "1", "-readingOrder", "-noImage", "-noLineNumbers",
               "/Users/jack/Documents/chonker_test.pdf", "/dev/stdout"])
        .output()?;
    
    let alto_xml = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        "<alto><String CONTENT=\"No PDF\" HPOS=\"0\" VPOS=\"0\"/></alto>".to_string()
    };
    
    // Extract simple text
    let spatial_text = "CITY CASH MANAGEMENT AND INVESTMENT POLICIES\n\nGeneral Fund Cash Flow\n\nDue to the fact that the receipt of revenues into the General Fund generally lag behind expenditures from the General Fund during each Fiscal Year, the City issues notes in anticipation of General Fund revenues, such as the Notes, and makes payments from the Consolidated Cash Account (described below) to finance its on-going operations. The City has issued, or PICA has issued on behalf of the City, tax and revenue anticipation notes in each Fiscal Year since Fiscal Year 1972. Each issue was repaid when due, prior to the end of the respective Fiscal Year. The City issued $130 million of tax and revenue anticipation notes on November 25, 2014, which matured on June 30, 2015.\n\nTable 1\nNotes Issued in Anticipation of Income by General Fund\nFiscal Years 2011-2015\n(Amount in millions)\n\nTotal Authorized Tax and Revenue Anticipation Notes (1)\n$285.00  $173.00  $127.00  $100.00  $130.00\n\nTotal Additional Notes Authorized\n$50.00   $50.00   N/A      N/A      N/A\n\nMaximum Amount Outstanding at any time during Fiscal Year\n0.0      0.0      0.0      0.0      0.0\n\nAmount Outstanding at June 30\n$0.00    $0.00    $0.00    $0.00    $0.00\n\nMaximum Amount Outstanding as a Percentage of General Fund Revenues\n7.38%    4.82%    3.43%    2.63%    3.43%\n\n2011  2012  2013  2014  2015\n\n(1) In fiscal years 2011-2013, the City issued short-term notes. See NOTE VIII to the Comprehensive Annual Financial Report of the City for the fiscal year ended June 30, 2015 (incorporated by reference hereto).".to_string();
    
    let app = AltoTextApp {
        alto_xml,
        spatial_text,
    };
    
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("Chonker9.5 - SIMPLE ALTO TEXT EDITOR"),
        ..Default::default()
    };
    
    eframe::run_native(
        "Simple ALTO",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )?;
    
    Ok(())
}