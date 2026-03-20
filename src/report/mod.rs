pub mod doctor;
pub mod json;
pub mod sarif;
pub mod terminal;

use crate::cli::OutputFormat;
use crate::model::ScanResult;

pub fn render(result: &ScanResult, format: &OutputFormat, no_color: bool, verbose: bool) -> String {
    match format {
        OutputFormat::Terminal => terminal::render(result, no_color, verbose),
        OutputFormat::Json => json::render(result),
        OutputFormat::Sarif => sarif::render(result),
    }
}
