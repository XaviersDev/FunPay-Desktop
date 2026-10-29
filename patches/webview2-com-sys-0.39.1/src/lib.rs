#[allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    clippy::all
)]
pub mod Microsoft {
    pub mod Web {
        pub mod WebView2 {
            pub mod Win32 {
                mod windows_core {
                    // FunPay Tools: под GNU не линкуем WebView2Loader.dll статически, а грузим при первом вызове.
                    // Иначе exe не запустится без DLL рядом. Приложение само кладёт DLL и загружает её заранее.
                    #[cfg(target_env = "msvc")]
                    macro_rules! link_webview2 {
                        ($library:literal $abi:literal fn $($function:tt)*) => (
                            #[link(name = "WebView2LoaderStatic", kind = "static")]
                            extern $abi {
                                pub fn $($function)*;
                            }
                        )
                    }
                    #[cfg(not(target_env = "msvc"))]
                    macro_rules! link_webview2 {
                        ($library:literal $abi:literal fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) -> $ret:ty) => (
                            #[allow(clippy::missing_safety_doc)]
                            pub unsafe fn $name($($arg: $ty),*) -> $ret {
                                type F = unsafe extern "system" fn($($ty),*) -> $ret;
                                match crate::dynload::proc(concat!(stringify!($name), "\0")) {
                                    Some(p) => {
                                        let f: F = core::mem::transmute(p);
                                        f($($arg),*)
                                    }
                                    None => windows_core::HRESULT(0x8007007Eu32 as i32),
                                }
                            }
                        )
                    }

                    pub(crate) use ::windows_core::*;
                    pub(crate) use link_webview2 as link;
                }

                include!("bindings.rs");
            }
        }
    }
}

pub mod declared_interfaces;

#[cfg(not(target_env = "msvc"))]
pub mod dynload {
    use std::sync::OnceLock;
    static H: OnceLock<usize> = OnceLock::new();
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> isize;
        fn GetProcAddress(h: isize, name: *const u8) -> usize;
    }
    /// Адрес функции из WebView2Loader.dll (по имени модуля; приложение загружает DLL заранее по полному пути).
    pub fn proc(name: &str) -> Option<usize> {
        let h = *H.get_or_init(|| {
            let w: Vec<u16> = "WebView2Loader.dll\0".encode_utf16().collect();
            unsafe { LoadLibraryW(w.as_ptr()) as usize }
        });
        if h == 0 {
            return None;
        }
        let p = unsafe { GetProcAddress(h as isize, name.as_ptr()) };
        if p == 0 { None } else { Some(p) }
    }
}

#[cfg(test)]
mod test {
    use windows_core::w;

    use crate::Microsoft::Web::WebView2::Win32::*;

    #[test]
    fn compare_eq() {
        let mut result = 1;
        unsafe { CompareBrowserVersions(w!("1.0.0"), w!("1.0.0"), &mut result) }.unwrap();
        assert_eq!(0, result);
    }

    #[test]
    fn compare_lt() {
        let mut result = 0;
        unsafe { CompareBrowserVersions(w!("1.0.0"), w!("1.0.1"), &mut result) }.unwrap();
        assert_eq!(-1, result);
    }

    #[test]
    fn compare_gt() {
        let mut result = 0;
        unsafe { CompareBrowserVersions(w!("2.0.0"), w!("1.0.1"), &mut result) }.unwrap();
        assert_eq!(1, result);
    }
}
