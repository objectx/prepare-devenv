//! Generate the UTF-16LE fixture bytes used by `src/capture.rs` tests.
//!
//! Run from the repo root with:
//!
//! ```text
//! cargo run --example gen_capture_fixtures -- tests/fixtures/capture/
//! ```
//!
//! The five `.bin` files this writes are committed alongside the example
//! source. Tests `include_bytes!` them directly rather than re-running the
//! generator on every test invocation, but keeping the generator under
//! source control means a maintainer can regenerate / extend the corpus
//! reproducibly without hand-crafting UTF-16LE byte sequences.

use std::path::PathBuf;

fn main() {
    let out_dir: PathBuf = std::env::args_os()
        .nth(1)
        .expect("usage: gen_capture_fixtures <out-dir>")
        .into();
    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    let marker = "__PREPARE_DEVENV_ENV_BEGIN_TEST__";

    // minimal_with_bom: BOM + chatty preamble + marker + 2 vars + CRLF
    {
        let mut s = String::new();
        s.push_str("Initializing developer command prompt...\r\n");
        s.push_str(marker);
        s.push_str("\r\n");
        s.push_str("FOO=bar\r\n");
        s.push_str("BAZ=qux\r\n");
        let bytes = encode_utf16le_with_bom(&s);
        std::fs::write(out_dir.join("minimal_with_bom.bin"), bytes).expect("write");
    }

    // minimal_no_bom: no BOM, LF-only line endings
    {
        let mut s = String::new();
        s.push_str(marker);
        s.push('\n');
        s.push_str("FOO=bar\n");
        let bytes = encode_utf16le_no_bom(&s);
        std::fs::write(out_dir.join("minimal_no_bom.bin"), bytes).expect("write");
    }

    // value_with_equals: parser must split on first `=` only
    {
        let s = format!("{marker}\r\nMYVAR=a=b=c\r\n");
        std::fs::write(
            out_dir.join("value_with_equals.bin"),
            encode_utf16le_with_bom(&s),
        )
        .expect("write");
    }

    // multibyte_non_ascii: BOM + marker + UTF-8/16 of Japanese text
    // (the fixture is encoded as UTF-16LE; the Japanese characters land
    // as their natural BMP codepoints).
    {
        let s = format!("{marker}\r\nWELCOME=こんにちは\r\nPATH=C:\\foo\r\n");
        std::fs::write(
            out_dir.join("multibyte_non_ascii.bin"),
            encode_utf16le_with_bom(&s),
        )
        .expect("write");
    }

    // missing_marker: no marker at all — parser must surface EnvParse.
    {
        let s = "FOO=bar\r\n".to_string();
        std::fs::write(
            out_dir.join("missing_marker.bin"),
            encode_utf16le_with_bom(&s),
        )
        .expect("write");
    }

    println!("wrote 5 fixtures to {}", out_dir.display());
}

fn encode_utf16le_with_bom(s: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in s.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn encode_utf16le_no_bom(s: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for unit in s.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}
