use serde::{Deserialize, Serialize};

/// Metadata and display properties for a fiat currency
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencyInfo {
    pub code: &'static str,
    pub name: &'static str,
    pub symbol: &'static str,
    pub flag_emoji: &'static str,
    pub decimals: u32,
}

pub static SUPPORTED_CURRENCIES: &[CurrencyInfo] = &[
    CurrencyInfo {
        code: "USD",
        name: "US Dollar",
        symbol: "$",
        flag_emoji: "🇺🇸",
        decimals: 2,
    },
    CurrencyInfo {
        code: "PHP",
        name: "Philippine Peso",
        symbol: "₱",
        flag_emoji: "🇵🇭",
        decimals: 2,
    },
    CurrencyInfo {
        code: "CAD",
        name: "Canadian Dollar",
        symbol: "C$",
        flag_emoji: "🇨🇦",
        decimals: 2,
    },
    CurrencyInfo {
        code: "JPY",
        name: "Japanese Yen",
        symbol: "¥",
        flag_emoji: "🇯🇵",
        decimals: 0,
    },
    CurrencyInfo {
        code: "EUR",
        name: "Euro",
        symbol: "€",
        flag_emoji: "🇪🇺",
        decimals: 2,
    },
    CurrencyInfo {
        code: "GBP",
        name: "British Pound",
        symbol: "£",
        flag_emoji: "🇬🇧",
        decimals: 2,
    },
    CurrencyInfo {
        code: "AUD",
        name: "Australian Dollar",
        symbol: "A$",
        flag_emoji: "🇦🇺",
        decimals: 2,
    },
    CurrencyInfo {
        code: "SGD",
        name: "Singapore Dollar",
        symbol: "S$",
        flag_emoji: "🇸🇬",
        decimals: 2,
    },
    CurrencyInfo {
        code: "INR",
        name: "Indian Rupee",
        symbol: "₹",
        flag_emoji: "🇮🇳",
        decimals: 2,
    },
    CurrencyInfo {
        code: "BRL",
        name: "Brazilian Real",
        symbol: "R$",
        flag_emoji: "🇧🇷",
        decimals: 2,
    },
    CurrencyInfo {
        code: "CHF",
        name: "Swiss Franc",
        symbol: "CHF",
        flag_emoji: "🇨🇭",
        decimals: 2,
    },
    CurrencyInfo {
        code: "NZD",
        name: "New Zealand Dollar",
        symbol: "NZ$",
        flag_emoji: "🇳🇿",
        decimals: 2,
    },
    CurrencyInfo {
        code: "HKD",
        name: "Hong Kong Dollar",
        symbol: "HK$",
        flag_emoji: "🇭🇰",
        decimals: 2,
    },
    CurrencyInfo {
        code: "KRW",
        name: "South Korean Won",
        symbol: "₩",
        flag_emoji: "🇰🇷",
        decimals: 0,
    },
    CurrencyInfo {
        code: "THB",
        name: "Thai Baht",
        symbol: "฿",
        flag_emoji: "🇹🇭",
        decimals: 2,
    },
    CurrencyInfo {
        code: "IDR",
        name: "Indonesian Rupiah",
        symbol: "Rp",
        flag_emoji: "🇮🇩",
        decimals: 0,
    },
    CurrencyInfo {
        code: "VND",
        name: "Vietnamese Dong",
        symbol: "₫",
        flag_emoji: "🇻🇳",
        decimals: 0,
    },
    CurrencyInfo {
        code: "MXN",
        name: "Mexican Peso",
        symbol: "Mex$",
        flag_emoji: "🇲🇽",
        decimals: 2,
    },
    CurrencyInfo {
        code: "AED",
        name: "UAE Dirham",
        symbol: "AED",
        flag_emoji: "🇦🇪",
        decimals: 2,
    },
];

/// Finds a currency by exact code, partial name, or known alias
pub fn find_currency(query: &str) -> Option<CurrencyInfo> {
    let q = query.trim().to_uppercase();
    if q.is_empty() {
        return None;
    }

    // Direct code match
    if let Some(c) = SUPPORTED_CURRENCIES.iter().find(|c| c.code == q) {
        return Some(*c);
    }

    // Alias matches
    match q.as_str() {
        "PESO" | "PESOS" | "PH" => find_currency("PHP"),
        "YEN" | "JP" => find_currency("JPY"),
        "POUND" | "POUNDS" | "QUID" | "UK" => find_currency("GBP"),
        "DOLLAR" | "BUCKS" | "US" => find_currency("USD"),
        "WON" | "KOREA" => find_currency("KRW"),
        "RUPEE" | "RUPEES" => find_currency("INR"),
        "BAHT" => find_currency("THB"),
        "REAL" | "REAIS" => find_currency("BRL"),
        "FRANC" => find_currency("CHF"),
        _ => {
            // Case-insensitive substring match on name
            let q_lower = query.trim().to_lowercase();
            SUPPORTED_CURRENCIES
                .iter()
                .find(|c| c.name.to_lowercase().contains(&q_lower))
                .copied()
        }
    }
}
