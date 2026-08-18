//! Round-trips a real Claude config through JsonDocument and diffs the result.
//! Usage: cargo run --example roundtrip_check -- <path-to-json>

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let src = std::env::args()
        .nth(1)
        .ok_or("usage: roundtrip_check <file>")?;
    let original = std::fs::read_to_string(&src)?;

    let scratch = std::env::temp_dir().join("byte-roundtrip-check.json");
    std::fs::write(&scratch, &original)?;

    let doc = byte::claude::document::JsonDocument::load(&scratch)?;
    let rewritten = String::from_utf8(doc.to_bytes()?)?;

    if original == rewritten {
        println!("IDENTICAL — {} bytes round-tripped exactly", original.len());
    } else {
        println!(
            "DIFFERS — original {} bytes, rewritten {} bytes",
            original.len(),
            rewritten.len()
        );
        for (i, (a, b)) in original.lines().zip(rewritten.lines()).enumerate() {
            if a != b {
                println!(
                    "first difference at line {}:\n  original:  {a}\n  rewritten: {b}",
                    i + 1
                );
                break;
            }
        }
    }
    std::fs::remove_file(&scratch).ok();
    Ok(())
}
