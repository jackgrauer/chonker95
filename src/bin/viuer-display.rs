use anyhow::Result;
use std::env;
use viuer::{Config, print_from_file};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <image_file>", args[0]);
        std::process::exit(1);
    }

    let image_path = &args[1];

    // Display image with viuer in terminal
    let config = Config {
        width: Some(40),
        height: Some(30),
        absolute_offset: false,
        ..Default::default()
    };

    print_from_file(image_path, &config)?;

    Ok(())
}