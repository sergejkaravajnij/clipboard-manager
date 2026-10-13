use crate::{i18n::Language, store::Result};
use std::path::Path;
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{Controls::*, Input::KeyboardAndMouse::*},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub key: u32,
}
impl Hotkey {
    pub fn validate(self) -> Result<()> {
        if self.key > 255 || self.modifiers & !(MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_WIN) != 0 {
            return Err("Invalid shortcut".into());
        }
        if self.key == 0 {
            return Ok(());
        }
        if self.key == VK_F12 as u32
            || [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN].contains(&(self.key as u16))
        {
            return Err("This key cannot be used as a shortcut".into());
        }
        if self.modifiers == 0 && !(VK_F1 as u32..=VK_F24 as u32).contains(&self.key) {
            return Err("Add Ctrl, Alt, Shift or Windows to the shortcut".into());
        }
        Ok(())
    }
    pub fn control_value(self) -> usize {
        let flags = if self.modifiers & MOD_CONTROL != 0 {
            HOTKEYF_CONTROL
        } else {
            0
        } | if self.modifiers & MOD_ALT != 0 {
            HOTKEYF_ALT
        } else {
            0
        } | if self.modifiers & MOD_SHIFT != 0 {
            HOTKEYF_SHIFT
        } else {
            0
        };
        self.key as usize | ((flags as usize) << 8)
    }
    pub fn from_control(value: usize, windows: bool) -> Self {
        let flags = (value >> 8) & 0xff;
        Self {
            key: (value & 0xff) as u32,
            modifiers: if flags & HOTKEYF_CONTROL as usize != 0 {
                MOD_CONTROL
            } else {
                0
            } | if flags & HOTKEYF_ALT as usize != 0 {
                MOD_ALT
            } else {
                0
            } | if flags & HOTKEYF_SHIFT as usize != 0 {
                MOD_SHIFT
            } else {
                0
            } | if windows { MOD_WIN } else { 0 },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub language: Language,
    pub hotkey: Hotkey,
    pub retention_days: u32,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::English,
            retention_days: 0,
            hotkey: Hotkey {
                modifiers: MOD_CONTROL | MOD_ALT,
                key: VK_V as u32,
            },
        }
    }
}
impl Preferences {
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let mut p = Self::default();
        for line in text.lines() {
            if let Some((name, value)) = line.split_once('=') {
                match name.trim() {
                    "language" => {
                        p.language = match value.trim() {
                            "en" => Language::English,
                            "ru" => Language::Russian,
                            _ => return Err("Invalid language in settings.ini".into()),
                        }
                    }
                    "hotkey_modifiers" => p.hotkey.modifiers = value.trim().parse()?,
                    "hotkey_key" => p.hotkey.key = value.trim().parse()?,
                    "retention_days" => p.retention_days = value.trim().parse()?,
                    _ => {}
                }
            }
        }
        p.hotkey.validate()?;
        Ok(p)
    }
    pub fn save(self, path: &Path) -> Result<()> {
        self.hotkey.validate()?;
        let temporary = path.with_extension("ini.tmp");
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary)?;
        write!(
            file,
            "language={}\nhotkey_modifiers={}\nhotkey_key={}\nretention_days={}\n",
            self.language.code(),
            self.hotkey.modifiers,
            self.hotkey.key,
            self.retention_days
        )?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(temporary, path)?;
        Ok(())
    }
}

pub struct Registration {
    pub hwnd: HWND,
    pub id: i32,
    pub shortcut: Option<Hotkey>,
}
impl Registration {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            id: 0,
            shortcut: None,
        }
    }
    pub fn apply(&mut self, settings: Preferences, path: Option<&Path>) -> Result<()> {
        settings.hotkey.validate()?;
        let target = if settings.hotkey.key == 0 {
            None
        } else {
            Some(settings.hotkey)
        };
        if target == self.shortcut {
            if let Some(path) = path {
                settings.save(path)?;
            }
            return Ok(());
        }
        let next_id = if self.id == 1 { 2 } else { 1 };
        if let Some(hotkey) = target
            && unsafe {
                RegisterHotKey(
                    self.hwnd,
                    next_id,
                    hotkey.modifiers | MOD_NOREPEAT,
                    hotkey.key,
                )
            } == 0
        {
            return Err("This shortcut is already in use or reserved by Windows. Choose another combination.".into());
        }
        if let Some(path) = path
            && let Err(e) = settings.save(path)
        {
            if target.is_some() {
                unsafe {
                    UnregisterHotKey(self.hwnd, next_id);
                }
            }
            return Err(e);
        }
        if self.id != 0 {
            unsafe {
                UnregisterHotKey(self.hwnd, self.id);
            }
        }
        self.shortcut = target;
        self.id = if target.is_some() { next_id } else { 0 };
        Ok(())
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        if self.id != 0 {
            unsafe {
                UnregisterHotKey(self.hwnd, self.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_both_languages_hotkey_and_disabled_state() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("clipboard-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("settings.ini");
        let mut p = Preferences {
            language: Language::Russian,
            retention_days: 30,
            hotkey: Hotkey {
                modifiers: MOD_CONTROL | MOD_SHIFT | MOD_WIN,
                key: VK_H as u32,
            },
        };
        p.save(&path)?;
        assert_eq!(Preferences::load(&path)?, p);
        p.language = Language::English;
        p.hotkey.key = 0;
        p.retention_days = 0;
        p.save(&path)?;
        assert_eq!(Preferences::load(&path)?, p);
        std::fs::write(&path, "language=ru\nhotkey_modifiers=3\nhotkey_key=86\n")?;
        assert_eq!(Preferences::load(&path)?.retention_days, 0);
        std::fs::remove_file(path)?;
        std::fs::remove_dir(dir)?;
        Ok(())
    }
    #[test]
    fn hotkey_control_roundtrip_and_validation() {
        for m in 0..16 {
            let h = Hotkey {
                modifiers: m,
                key: VK_V as u32,
            };
            assert_eq!(Hotkey::from_control(h.control_value(), m & MOD_WIN != 0), h);
        }
        assert!(
            Hotkey {
                modifiers: 0,
                key: VK_A as u32
            }
            .validate()
            .is_err()
        );
        assert!(
            Hotkey {
                modifiers: MOD_CONTROL,
                key: VK_F12 as u32
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn conflicting_shortcut_and_failed_save_keep_previous_registration() -> Result<()> {
        let mut active = Registration::new(std::ptr::null_mut());
        let mut chosen = None;
        for key in VK_F18..=VK_F24 {
            let p = Preferences {
                hotkey: Hotkey {
                    modifiers: MOD_CONTROL | MOD_ALT | MOD_SHIFT,
                    key: key as u32,
                },
                ..Default::default()
            };
            if active.apply(p, None).is_ok() {
                chosen = Some(p);
                break;
            }
        }
        let p = chosen.ok_or("No available test shortcut")?;
        // Use a different explicit ID: registrations with the same HWND/ID are additive.
        assert_eq!(
            unsafe { RegisterHotKey(std::ptr::null_mut(), 101, p.hotkey.modifiers, p.hotkey.key) },
            0
        );
        let before = active.shortcut;
        let candidate = Preferences {
            hotkey: Hotkey {
                modifiers: MOD_CONTROL | MOD_ALT | MOD_SHIFT,
                key: VK_F17 as u32,
            },
            ..p
        };
        assert!(
            active
                .apply(
                    candidate,
                    Some(Path::new(
                        "Z:\\nonexistent-clipboard-settings\\settings.ini"
                    ))
                )
                .is_err()
        );
        assert_eq!(active.shortcut, before);
        Ok(())
    }
}
