//! A live CDN probe for the read-only verification protocol.
//!
//! ```text
//! ow-casc-cdn-fetch index <hash>                  # fetch <hash>.index; compare with the local copy
//! ow-casc-cdn-fetch loose <key-hex-32>            # whole GET of a loose file (its own key route)
//! ow-casc-cdn-fetch range <key-hex-32> OFF LEN    # Range GET; prints length and magic
//! ```
//!
//! Needs `OW_CDN=1` in the environment, like every fetch path. Writes only
//! under the CDN cache directory (`OW_CDN_CACHE`, default `target/cdn-cache`);
//! read-only toward the install.

use std::path::Path;
use std::process::ExitCode;

use ow_casc::install::detect_root;
use ow_casc::Install;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(mode) = args.first().map(String::as_str) else {
        eprintln!(
            "usage: ow-casc-cdn-fetch (index <hash> | loose <key> | range <key> <offset> <len>)"
        );
        return ExitCode::from(2);
    };
    let root = match detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL");
            return ExitCode::from(2);
        }
    };
    let install = match Install::open_with_support_keyring(&root, None) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("install: {e}");
            return ExitCode::from(1);
        }
    };
    if install.cdn.is_none() {
        eprintln!("OW_CDN is not set to a true value; refusing to fetch");
        return ExitCode::from(2);
    }
    match run(mode, &args, &root, &install) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn run(mode: &str, args: &[String], root: &Path, install: &Install) -> Result<(), String> {
    let cdn = install.cdn.as_ref().expect("checked by the caller");
    match mode {
        "index" => {
            let hash = arg(args, 1)?;
            // The reference's suffix convention is raw concatenation: the
            // index fetch passes ".index" with its dot (ICDNClient.FetchIndexFile).
            let fetched = cdn
                .fetch_data(hash, Some(".index"))
                .map_err(|e| e.to_string())?;
            let local_path = root
                .join("data")
                .join("casc")
                .join("indices")
                .join(format!("{hash}.index"));
            let local =
                std::fs::read(&local_path).map_err(|e| format!("{}: {e}", local_path.display()))?;
            println!(
                "index {hash}: fetched {} B, local {} B",
                fetched.len(),
                local.len()
            );
            if fetched == local {
                println!("byte-for-byte equal: yes");
            } else {
                let first = fetched.iter().zip(&local).position(|(a, b)| a != b);
                println!(
                    "byte-for-byte equal: NO (len {} vs {}, first difference {first:?})",
                    fetched.len(),
                    local.len()
                );
            }
            Ok(())
        }
        "loose" => {
            let key = arg(args, 1)?;
            let bytes = cdn.fetch_data(key, None).map_err(|e| e.to_string())?;
            dump("loose", key, &bytes);
            Ok(())
        }
        "range" => {
            let key = arg(args, 1)?;
            let offset: u64 = arg(args, 2)?
                .parse()
                .map_err(|_| "offset is not a number".to_string())?;
            let len: u64 = arg(args, 3)?
                .parse()
                .map_err(|_| "len is not a number".to_string())?;
            let bytes = cdn
                .fetch_range(key, offset, len)
                .map_err(|e| e.to_string())?;
            dump("range", key, &bytes);
            Ok(())
        }
        other => Err(format!("unknown mode {other:?}")),
    }
}

fn arg(args: &[String], i: usize) -> Result<&str, String> {
    args.get(i)
        .map(String::as_str)
        .ok_or_else(|| format!("missing argument {i}"))
}

fn dump(mode: &str, key: &str, bytes: &[u8]) {
    let head = &bytes[..bytes.len().min(8)];
    println!(
        "{mode} {key}: {} B, first bytes {head:02x?} ({:?})",
        bytes.len(),
        String::from_utf8_lossy(&bytes[..bytes.len().min(4)])
    );
}
