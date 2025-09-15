use anyhow::Result;
use pdfium_render::prelude::*;
use std::env;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 4 {
        eprintln!("Usage: {} <pdf_file> <page_number> <output_image>", args[0]);
        std::process::exit(1);
    }

    let pdf_path = &args[1];
    let page_num: u32 = args[2].parse().unwrap_or(1);
    let output_path = &args[3];

    // Load PDF and extract page
    let pdfium = Pdfium::default();
    let document = pdfium.load_pdf_from_file(pdf_path, None)?;
    let page_index = (page_num - 1) as u16;

    if page_index >= document.pages().len() {
        eprintln!("Page {} not found in PDF", page_num);
        std::process::exit(1);
    }

    let page = document.pages().get(page_index)?;

    // Render page to image
    let render_config = PdfRenderConfig::new()
        .set_target_width(800)
        .set_target_height(1000);

    let bitmap = page.render_with_config(&render_config)?;
    let image_data = bitmap.as_image();

    // Save as PNG
    image_data.save(output_path)?;

    println!("Converted page {} to {}", page_num, output_path);
    Ok(())
}