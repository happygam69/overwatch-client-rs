//! Stages D1–D3 smoke run: create the client's D3D11 device and swap chain on
//! a hidden window, create the D2 swap-chain views, run the D3 step list for
//! a few frames, then resize and run one more.
//!
//! ```text
//! cargo run -p ow-render --example d3d11_smoke
//! ```
//!
//! This does not need RenderDoc or PIX. A failure prints the failing call and
//! its HRESULT; the creation path is then reported as unsupported on this
//! machine rather than faked.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HMENU, WINDOW_EX_STYLE, WS_POPUP,
    };

    use ow_render::request::{DeviceRequest, SwapChainRequest};
    use ow_render::{frame, WindowSurface};

    const WIDTH: u32 = 1280;
    const HEIGHT: u32 = 720;

    let module = unsafe { GetModuleHandleW(PCWSTR::null())? };
    let hwnd: HWND = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("ow-render D1-D3 smoke"),
            WS_POPUP,
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            HWND(0),
            HMENU(0),
            HINSTANCE(module.0),
            None,
        )
    };
    if hwnd.0 == 0 {
        return Err("CreateWindowExW returned a null HWND".into());
    }

    // Diagnostic overrides, so the creation path can be exercised under a
    // capture tool without editing the read values: `OW_D3D11_LEVEL=b000`
    // asks for one feature level, `OW_D3D11_ALLOW_LAYER=1` drops the
    // PREVENT_ALTERING_LAYER flag, `OW_D3D11_SINGLE=1` sets SINGLETHREADED.
    let mut device_request = DeviceRequest::default();
    if let Ok(level) = std::env::var("OW_D3D11_LEVEL") {
        let level = level.trim_start_matches("0x");
        device_request.feature_level_override = Some(
            u32::from_str_radix(level, 16).expect("OW_D3D11_LEVEL must be a hex feature level"),
        );
    }
    if std::env::var("OW_D3D11_ALLOW_LAYER").is_ok() {
        device_request.allow_layer_altering = true;
    }
    if std::env::var("OW_D3D11_SINGLE").is_ok() {
        device_request.single_threaded = true;
    }
    let swap_request = SwapChainRequest::client_default(WIDTH, HEIGHT, 1);
    println!("D1 device request:");
    println!(
        "  feature levels: primary {:?}, fallback {:?}, override {:?}",
        device_request.primary_feature_levels(),
        device_request.fallback_feature_levels(),
        device_request.feature_level_override
    );
    println!(
        "  flag inputs: debug={} single_threaded={} allow_layer_altering={} -> flags=0x{:02x}",
        device_request.debug_layer,
        device_request.single_threaded,
        device_request.allow_layer_altering,
        device_request.creation_flags()
    );
    println!(
        "  (the three flag inputs and the sync interval are runtime config values; their shipped defaults are open)"
    );

    let result = WindowSurface::new(hwnd.0, device_request, swap_request);
    let mut surface = match result {
        Ok(surface) => surface,
        Err(error) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            eprintln!("D1 creation failed: {error}");
            if let Some(hr) = error.hresult() {
                eprintln!("HRESULT: 0x{:08x}", hr);
            }
            return Err(error.into());
        }
    };

    // Optional RenderDoc capture of the D1-D3 path. The client's
    // PREVENT_ALTERING_LAYER flag (0x80) makes RenderDoc return a null
    // immediate context, so a capture run must set OW_D3D11_ALLOW_LAYER=1;
    // that input's shipped value is unread, so this changes no read value.
    let capture = match std::env::var("OW_RENDERDOC_CAPTURE") {
        Ok(template) => {
            let capture = capture::attach(&template);
            if capture.is_none() {
                eprintln!("renderdoc: attach failed (is renderdoc.dll injected?)");
            }
            capture
        }
        Err(_) => {
            println!("renderdoc: OW_RENDERDOC_CAPTURE not set; no capture");
            None
        }
    };
    if let Some(capture) = &capture {
        capture.trigger();
        println!(
            "renderdoc: capture triggered into {template}",
            template = capture.template()
        );
    }

    println!("\nD1 created values:");
    println!("{}", surface.describe());
    println!(
        "\nclear colour (D1 placeholder, not read): {:?}",
        ow_render::D1_CLEAR_COLOUR
    );

    println!("\nD2 swap-chain views:");
    surface.log_views()?;

    println!("\nD3 frame order, two frames:");
    frame::run_frames(&mut surface, 0, 2, true)?;
    println!("presented twice");

    surface.resize(640, 360)?;
    println!("\nD2 views after resize:");
    surface.log_views()?;
    println!("\nD3 frame order, one frame after resize:");
    frame::run_frames(&mut surface, 2, 1, true)?;

    if let Some(capture) = capture {
        capture.wait_for_capture();
    }

    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    Ok(())
}

/// Minimal in-application RenderDoc API: enough to set the capture path and
/// trigger one frame. This is a diagnostic for the capture oracle, not part
/// of the renderer. See `renderdoc_app.h` for the v1.6.0 table order.
#[cfg(windows)]
mod capture {
    use std::ffi::{c_char, c_void, CString};

    use windows::core::{s, w};
    use windows::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    const API_1_6_0: u32 = 10600;
    const SET_PATH_TEMPLATE: usize = 11;
    const GET_NUM_CAPTURES: usize = 13;
    const TRIGGER_CAPTURE: usize = 15;

    pub struct Capture {
        library: HMODULE,
        api: *mut c_void,
        template: String,
    }

    impl Capture {
        pub fn template(&self) -> &str {
            &self.template
        }

        pub fn trigger(&self) {
            unsafe {
                let trigger: extern "C" fn() = std::mem::transmute(self.slot(TRIGGER_CAPTURE));
                trigger();
            }
        }

        pub fn wait_for_capture(&self) {
            for _ in 0..200 {
                if unsafe { self.captures() } > 0 {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }

        unsafe fn captures(&self) -> u32 {
            let get: extern "C" fn() -> u32 = std::mem::transmute(self.slot(GET_NUM_CAPTURES));
            get()
        }

        unsafe fn slot(&self, index: usize) -> *const c_void {
            *((self.api as *const *const c_void).add(index))
        }
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            unsafe {
                let _ = FreeLibrary(self.library);
            }
        }
    }

    pub fn attach(template: &str) -> Option<Capture> {
        unsafe {
            let library = match LoadLibraryW(w!("renderdoc.dll")) {
                Ok(library) => library,
                Err(error) => {
                    eprintln!("renderdoc: LoadLibraryW failed: {error}");
                    return None;
                }
            };
            let get_api = match GetProcAddress(library, s!("RENDERDOC_GetAPI")) {
                Some(get_api) => get_api,
                None => {
                    eprintln!("renderdoc: RENDERDOC_GetAPI not exported");
                    let _ = FreeLibrary(library);
                    return None;
                }
            };
            let get_api: extern "C" fn(u32, *mut *mut c_void) -> i32 = std::mem::transmute(get_api);
            let mut api: *mut c_void = std::ptr::null_mut();
            if get_api(API_1_6_0, &mut api) == 0 || api.is_null() {
                eprintln!("renderdoc: RENDERDOC_GetAPI returned no API (not injected?)");
                let _ = FreeLibrary(library);
                return None;
            }
            let capture = Capture {
                library,
                api,
                template: template.to_string(),
            };
            let path = CString::new(template).ok()?;
            let set_path: extern "C" fn(*const c_char) =
                std::mem::transmute(capture.slot(SET_PATH_TEMPLATE));
            set_path(path.as_ptr());
            Some(capture)
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("ow-render is a Windows D3D11 backend; this example needs Windows");
}
