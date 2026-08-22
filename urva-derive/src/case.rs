pub fn to_snake(ident: &str) -> String {
    let chars: Vec<char> = ident.chars().collect();
    let mut out = String::new();
    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() && i != 0 {
            let prev = chars[i - 1];
            let next_is_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
            if prev != '_' && (!prev.is_uppercase() || next_is_lower) {
                out.push('_');
            }
        }
        out.extend(ch.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::to_snake;

    #[test]
    fn namespaces_follow_rust_module_naming() {
        assert_eq!(to_snake("Order"), "order");
        assert_eq!(to_snake("LedgerEntry"), "ledger_entry");
        assert_eq!(to_snake("HTTPServer"), "http_server");
        assert_eq!(to_snake("Shipping2D"), "shipping2_d");
        assert_eq!(to_snake("already_snake"), "already_snake");
    }
}
