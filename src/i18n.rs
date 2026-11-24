//! Localized strings: ClashBar's `Localizable.strings` (zh-Hans and en),
//! as JSON, with `%@` / `%d` placeholders filled in order.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::settings::AppLanguage;

fn table(language: AppLanguage) -> &'static HashMap<String, String> {
    static ZH: OnceLock<HashMap<String, String>> = OnceLock::new();
    static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
    match language {
        AppLanguage::ZhHans => {
            ZH.get_or_init(|| serde_json::from_str(include_str!("i18n/zh-Hans.json")).unwrap_or_default())
        }
        AppLanguage::En => EN.get_or_init(|| serde_json::from_str(include_str!("i18n/en.json")).unwrap_or_default()),
    }
}

/// The string for `key` (the key itself when missing), placeholders filled
/// with `args` in order.
pub fn tr(language: AppLanguage, key: &str, args: &[&dyn std::fmt::Display]) -> String {
    let template = table(language).get(key).map(String::as_str).unwrap_or(key);
    if args.is_empty() {
        return template.to_owned();
    }
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut chars = template.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '%' {
            if let Some(&(_, spec)) = chars.peek() {
                if spec == '@' || spec == 'd' {
                    chars.next();
                    if let Some(arg) = args.get(next) {
                        out.push_str(&arg.to_string());
                    }
                    next += 1;
                    continue;
                }
                if spec == '%' {
                    chars.next();
                    out.push('%');
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_placeholders() {
        assert_eq!(tr(AppLanguage::ZhHans, "ui.tab.proxy", &[]), "节点");
        assert_eq!(tr(AppLanguage::En, "log.process.terminated", &[&3]), "mihomo terminated with code 3");
        assert_eq!(tr(AppLanguage::ZhHans, "missing.key", &[]), "missing.key");
    }
}
