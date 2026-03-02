use anyhow::{Context, Result, bail};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_SNAPSHOT,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Hotkey {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub vk: u32,
}

pub fn parse_hotkey(input: &str) -> Result<Hotkey> {
    let mut modifiers = HOT_KEY_MODIFIERS(0);
    let mut key = None::<u32>;

    for raw_token in input.split('+') {
        let token = raw_token.trim();
        if token.is_empty() {
            bail!("hotkey token cannot be empty");
        }

        let token_upper = token.to_ascii_uppercase();
        match token_upper.as_str() {
            "ALT" => modifiers |= MOD_ALT,
            "CTRL" | "CONTROL" => modifiers |= MOD_CONTROL,
            "SHIFT" => modifiers |= MOD_SHIFT,
            "WIN" | "WINDOWS" | "META" => modifiers |= MOD_WIN,
            _ => {
                if key.is_some() {
                    bail!("hotkey must include exactly one non-modifier key");
                }
                key = Some(parse_virtual_key(&token_upper)?);
            }
        }
    }

    let vk = key.context("hotkey missing key")?;
    if modifiers.0 == 0 && vk != VK_SNAPSHOT.0 as u32 {
        bail!("hotkey must include at least one modifier (except PrintScreen)");
    }
    let modifiers = HOT_KEY_MODIFIERS(modifiers.0 | MOD_NOREPEAT.0);
    Ok(Hotkey { modifiers, vk })
}

fn parse_virtual_key(token: &str) -> Result<u32> {
    if token.len() == 1 {
        let ch = token.chars().next().expect("len checked");
        if ch.is_ascii_uppercase() || ch.is_ascii_digit() {
            return Ok(ch as u32);
        }
    }

    if let Some(number) = token.strip_prefix('F') {
        if let Ok(value) = number.parse::<u32>() {
            if (1..=24).contains(&value) {
                return Ok(111 + value);
            }
        }
    }

    match token {
        "PRINTSCREEN" | "PRTSC" | "PRTSCN" | "SNAPSHOT" => Ok(VK_SNAPSHOT.0 as u32),
        _ => bail!("unsupported key `{token}`"),
    }
}
#[cfg(test)]
mod tests {
    use super::parse_hotkey;

    #[test]
    fn parses_default_hotkey() {
        let parsed = parse_hotkey("Alt+Shift+S").expect("valid hotkey");
        assert_eq!(parsed.vk, 'S' as u32);
        assert_ne!(parsed.modifiers.0, 0);
    }

    #[test]
    fn rejects_missing_modifier() {
        let err = parse_hotkey("S").expect_err("must fail");
        assert!(err.to_string().contains("modifier"));
    }

    #[test]
    fn allows_printscreen_without_modifier() {
        let parsed = parse_hotkey("PrintScreen").expect("valid hotkey");
        assert_eq!(parsed.vk, super::VK_SNAPSHOT.0 as u32);
    }

    #[test]
    fn parses_function_key() {
        let parsed = parse_hotkey("Ctrl+F12").expect("valid hotkey");
        assert_eq!(parsed.vk, 123);
    }
}
