//! Register the `bm2dxinf://` URI scheme handler.

use anyhow::Result;

pub fn run() -> Result<()> {
    infst::launcher::register_uri_scheme()?;
    println!("URI scheme 'bm2dxinf://' registered successfully.");
    println!("You can now launch INFINITAS from your browser.");
    Ok(())
}
