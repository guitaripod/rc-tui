/// Regional indicator symbol letter A, the base for two-letter flag emoji.
const REGIONAL_INDICATOR_A: u32 = 0x1F1E6;

/// ISO 3166-1 alpha-2 codes with everyday English names, sorted by code.
const COUNTRIES: [(&str, &str); 250] = [
    ("AD", "Andorra"),
    ("AE", "United Arab Emirates"),
    ("AF", "Afghanistan"),
    ("AG", "Antigua and Barbuda"),
    ("AI", "Anguilla"),
    ("AL", "Albania"),
    ("AM", "Armenia"),
    ("AO", "Angola"),
    ("AQ", "Antarctica"),
    ("AR", "Argentina"),
    ("AS", "American Samoa"),
    ("AT", "Austria"),
    ("AU", "Australia"),
    ("AW", "Aruba"),
    ("AX", "Åland Islands"),
    ("AZ", "Azerbaijan"),
    ("BA", "Bosnia and Herzegovina"),
    ("BB", "Barbados"),
    ("BD", "Bangladesh"),
    ("BE", "Belgium"),
    ("BF", "Burkina Faso"),
    ("BG", "Bulgaria"),
    ("BH", "Bahrain"),
    ("BI", "Burundi"),
    ("BJ", "Benin"),
    ("BL", "Saint Barthélemy"),
    ("BM", "Bermuda"),
    ("BN", "Brunei"),
    ("BO", "Bolivia"),
    ("BQ", "Caribbean Netherlands"),
    ("BR", "Brazil"),
    ("BS", "Bahamas"),
    ("BT", "Bhutan"),
    ("BV", "Bouvet Island"),
    ("BW", "Botswana"),
    ("BY", "Belarus"),
    ("BZ", "Belize"),
    ("CA", "Canada"),
    ("CC", "Cocos (Keeling) Islands"),
    ("CD", "Congo - Kinshasa"),
    ("CF", "Central African Republic"),
    ("CG", "Congo - Brazzaville"),
    ("CH", "Switzerland"),
    ("CI", "Côte d'Ivoire"),
    ("CK", "Cook Islands"),
    ("CL", "Chile"),
    ("CM", "Cameroon"),
    ("CN", "China"),
    ("CO", "Colombia"),
    ("CR", "Costa Rica"),
    ("CU", "Cuba"),
    ("CV", "Cape Verde"),
    ("CW", "Curaçao"),
    ("CX", "Christmas Island"),
    ("CY", "Cyprus"),
    ("CZ", "Czechia"),
    ("DE", "Germany"),
    ("DJ", "Djibouti"),
    ("DK", "Denmark"),
    ("DM", "Dominica"),
    ("DO", "Dominican Republic"),
    ("DZ", "Algeria"),
    ("EC", "Ecuador"),
    ("EE", "Estonia"),
    ("EG", "Egypt"),
    ("EH", "Western Sahara"),
    ("ER", "Eritrea"),
    ("ES", "Spain"),
    ("ET", "Ethiopia"),
    ("FI", "Finland"),
    ("FJ", "Fiji"),
    ("FK", "Falkland Islands"),
    ("FM", "Micronesia"),
    ("FO", "Faroe Islands"),
    ("FR", "France"),
    ("GA", "Gabon"),
    ("GB", "United Kingdom"),
    ("GD", "Grenada"),
    ("GE", "Georgia"),
    ("GF", "French Guiana"),
    ("GG", "Guernsey"),
    ("GH", "Ghana"),
    ("GI", "Gibraltar"),
    ("GL", "Greenland"),
    ("GM", "Gambia"),
    ("GN", "Guinea"),
    ("GP", "Guadeloupe"),
    ("GQ", "Equatorial Guinea"),
    ("GR", "Greece"),
    ("GS", "South Georgia and the South Sandwich Islands"),
    ("GT", "Guatemala"),
    ("GU", "Guam"),
    ("GW", "Guinea-Bissau"),
    ("GY", "Guyana"),
    ("HK", "Hong Kong"),
    ("HM", "Heard Island and McDonald Islands"),
    ("HN", "Honduras"),
    ("HR", "Croatia"),
    ("HT", "Haiti"),
    ("HU", "Hungary"),
    ("ID", "Indonesia"),
    ("IE", "Ireland"),
    ("IL", "Israel"),
    ("IM", "Isle of Man"),
    ("IN", "India"),
    ("IO", "British Indian Ocean Territory"),
    ("IQ", "Iraq"),
    ("IR", "Iran"),
    ("IS", "Iceland"),
    ("IT", "Italy"),
    ("JE", "Jersey"),
    ("JM", "Jamaica"),
    ("JO", "Jordan"),
    ("JP", "Japan"),
    ("KE", "Kenya"),
    ("KG", "Kyrgyzstan"),
    ("KH", "Cambodia"),
    ("KI", "Kiribati"),
    ("KM", "Comoros"),
    ("KN", "Saint Kitts and Nevis"),
    ("KP", "North Korea"),
    ("KR", "South Korea"),
    ("KW", "Kuwait"),
    ("KY", "Cayman Islands"),
    ("KZ", "Kazakhstan"),
    ("LA", "Laos"),
    ("LB", "Lebanon"),
    ("LC", "Saint Lucia"),
    ("LI", "Liechtenstein"),
    ("LK", "Sri Lanka"),
    ("LR", "Liberia"),
    ("LS", "Lesotho"),
    ("LT", "Lithuania"),
    ("LU", "Luxembourg"),
    ("LV", "Latvia"),
    ("LY", "Libya"),
    ("MA", "Morocco"),
    ("MC", "Monaco"),
    ("MD", "Moldova"),
    ("ME", "Montenegro"),
    ("MF", "Saint Martin"),
    ("MG", "Madagascar"),
    ("MH", "Marshall Islands"),
    ("MK", "North Macedonia"),
    ("ML", "Mali"),
    ("MM", "Myanmar"),
    ("MN", "Mongolia"),
    ("MO", "Macao"),
    ("MP", "Northern Mariana Islands"),
    ("MQ", "Martinique"),
    ("MR", "Mauritania"),
    ("MS", "Montserrat"),
    ("MT", "Malta"),
    ("MU", "Mauritius"),
    ("MV", "Maldives"),
    ("MW", "Malawi"),
    ("MX", "Mexico"),
    ("MY", "Malaysia"),
    ("MZ", "Mozambique"),
    ("NA", "Namibia"),
    ("NC", "New Caledonia"),
    ("NE", "Niger"),
    ("NF", "Norfolk Island"),
    ("NG", "Nigeria"),
    ("NI", "Nicaragua"),
    ("NL", "Netherlands"),
    ("NO", "Norway"),
    ("NP", "Nepal"),
    ("NR", "Nauru"),
    ("NU", "Niue"),
    ("NZ", "New Zealand"),
    ("OM", "Oman"),
    ("PA", "Panama"),
    ("PE", "Peru"),
    ("PF", "French Polynesia"),
    ("PG", "Papua New Guinea"),
    ("PH", "Philippines"),
    ("PK", "Pakistan"),
    ("PL", "Poland"),
    ("PM", "Saint Pierre and Miquelon"),
    ("PN", "Pitcairn Islands"),
    ("PR", "Puerto Rico"),
    ("PS", "Palestine"),
    ("PT", "Portugal"),
    ("PW", "Palau"),
    ("PY", "Paraguay"),
    ("QA", "Qatar"),
    ("RE", "Réunion"),
    ("RO", "Romania"),
    ("RS", "Serbia"),
    ("RU", "Russia"),
    ("RW", "Rwanda"),
    ("SA", "Saudi Arabia"),
    ("SB", "Solomon Islands"),
    ("SC", "Seychelles"),
    ("SD", "Sudan"),
    ("SE", "Sweden"),
    ("SG", "Singapore"),
    ("SH", "Saint Helena, Ascension and Tristan da Cunha"),
    ("SI", "Slovenia"),
    ("SJ", "Svalbard and Jan Mayen"),
    ("SK", "Slovakia"),
    ("SL", "Sierra Leone"),
    ("SM", "San Marino"),
    ("SN", "Senegal"),
    ("SO", "Somalia"),
    ("SR", "Suriname"),
    ("SS", "South Sudan"),
    ("ST", "São Tomé and Príncipe"),
    ("SV", "El Salvador"),
    ("SX", "Sint Maarten"),
    ("SY", "Syria"),
    ("SZ", "Eswatini"),
    ("TC", "Turks and Caicos Islands"),
    ("TD", "Chad"),
    ("TF", "French Southern Territories"),
    ("TG", "Togo"),
    ("TH", "Thailand"),
    ("TJ", "Tajikistan"),
    ("TK", "Tokelau"),
    ("TL", "Timor-Leste"),
    ("TM", "Turkmenistan"),
    ("TN", "Tunisia"),
    ("TO", "Tonga"),
    ("TR", "Türkiye"),
    ("TT", "Trinidad and Tobago"),
    ("TV", "Tuvalu"),
    ("TW", "Taiwan"),
    ("TZ", "Tanzania"),
    ("UA", "Ukraine"),
    ("UG", "Uganda"),
    ("UM", "United States Minor Outlying Islands"),
    ("US", "United States"),
    ("UY", "Uruguay"),
    ("UZ", "Uzbekistan"),
    ("VA", "Vatican City"),
    ("VC", "Saint Vincent and the Grenadines"),
    ("VE", "Venezuela"),
    ("VG", "British Virgin Islands"),
    ("VI", "U.S. Virgin Islands"),
    ("VN", "Vietnam"),
    ("VU", "Vanuatu"),
    ("WF", "Wallis and Futuna"),
    ("WS", "Samoa"),
    ("XK", "Kosovo"),
    ("YE", "Yemen"),
    ("YT", "Mayotte"),
    ("ZA", "South Africa"),
    ("ZM", "Zambia"),
    ("ZW", "Zimbabwe"),
];

/// Alternative spellings mapped to ISO alpha-2 codes.
const ALIASES: [(&str, &str); 45] = [
    ("Korea, Republic of", "KR"),
    ("Republic of Korea", "KR"),
    ("Korea", "KR"),
    ("Korea, Democratic People's Republic of", "KP"),
    ("Turkey", "TR"),
    ("Czech Republic", "CZ"),
    ("Viet Nam", "VN"),
    ("Russian Federation", "RU"),
    ("Hong Kong SAR China", "HK"),
    ("Hong Kong SAR", "HK"),
    ("Macau", "MO"),
    ("Macao SAR China", "MO"),
    ("Taiwan, Province of China", "TW"),
    ("Iran, Islamic Republic of", "IR"),
    ("Syrian Arab Republic", "SY"),
    ("Lao People's Democratic Republic", "LA"),
    ("Moldova, Republic of", "MD"),
    ("Bolivia, Plurinational State of", "BO"),
    ("Venezuela, Bolivarian Republic of", "VE"),
    ("Tanzania, United Republic of", "TZ"),
    ("Macedonia", "MK"),
    ("Swaziland", "SZ"),
    ("Burma", "MM"),
    ("Cabo Verde", "CV"),
    ("Ivory Coast", "CI"),
    ("Cote d'Ivoire", "CI"),
    ("Holy See", "VA"),
    ("Palestine, State of", "PS"),
    ("Brunei Darussalam", "BN"),
    ("Micronesia, Federated States of", "FM"),
    ("United States of America", "US"),
    ("USA", "US"),
    ("UK", "GB"),
    ("Great Britain", "GB"),
    ("Democratic Republic of the Congo", "CD"),
    ("Republic of the Congo", "CG"),
    ("The Netherlands", "NL"),
    ("UAE", "AE"),
    ("Mainland China", "CN"),
    ("People's Republic of China", "CN"),
    ("Saint Kitts & Nevis", "KN"),
    ("St. Kitts and Nevis", "KN"),
    ("Gambia, The", "GM"),
    ("Bahamas, The", "BS"),
    ("Eswatini, Kingdom of", "SZ"),
];

/// Returns the short English name for a two-letter country code, ignoring case and surrounding whitespace.
pub fn name(code: &str) -> Option<&'static str> {
    let key = code.trim();
    COUNTRIES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(key))
        .map(|(_, name)| *name)
}

/// Returns the alpha-2 code for a country name or alias, or for a bare two-letter code.
pub fn code(name: &str) -> Option<&'static str> {
    let key = fold(name);
    COUNTRIES
        .iter()
        .find(|(_, known)| fold(known) == key)
        .map(|(code, _)| *code)
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(alias, _)| fold(alias) == key)
                .map(|(_, code)| *code)
        })
        .or_else(|| bare_code(name))
}

/// Returns the regional indicator emoji pair for a two-letter ASCII code, or an empty string otherwise.
pub fn flag(code: &str) -> String {
    let code = code.trim();
    if code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        code.to_ascii_uppercase()
            .bytes()
            .filter_map(|byte| char::from_u32(REGIONAL_INDICATOR_A + u32::from(byte - b'A')))
            .collect()
    } else {
        String::new()
    }
}

/// Returns the flag and English name for a known code, such as "🇫🇮 Finland", or the uppercased code when unknown.
pub fn label(code: &str) -> String {
    let upper = code.trim().to_ascii_uppercase();
    match name(&upper) {
        Some(name) => format!("{} {name}", flag(&upper)),
        None => upper,
    }
}

/// Lowercases text, maps common accented letters to plain ASCII and keeps only alphanumeric characters.
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|character| match character {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            other => other,
        })
        .filter(|character| character.is_alphanumeric())
        .collect()
}

/// Resolves a bare two-letter ASCII code against the country table.
fn bare_code(text: &str) -> Option<&'static str> {
    let trimmed = text.trim();
    if trimmed.len() == 2 && trimmed.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        COUNTRIES
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(trimmed))
            .map(|(known, _)| *known)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_two_uppercase_ascii_letters_and_unique() {
        assert!(
            COUNTRIES
                .iter()
                .all(|(code, _)| code.len() == 2
                    && code.bytes().all(|byte| byte.is_ascii_uppercase()))
        );
        assert!(COUNTRIES.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn codes_are_sorted() {
        let codes: Vec<&str> = COUNTRIES.iter().map(|(code, _)| *code).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted);
    }

    #[test]
    fn table_has_at_least_250_entries() {
        assert!(COUNTRIES.len() >= 250);
    }

    #[test]
    fn every_name_maps_back_to_its_own_code() {
        for &(code_value, country) in COUNTRIES.iter() {
            assert_eq!(super::code(country), Some(code_value), "{country}");
        }
    }

    #[test]
    fn every_alias_points_to_a_listed_code() {
        for &(alias, code_value) in ALIASES.iter() {
            assert!(
                COUNTRIES.iter().any(|(known, _)| *known == code_value),
                "{alias} -> {code_value}"
            );
            assert_eq!(super::code(alias), Some(code_value), "{alias}");
        }
    }

    #[test]
    fn name_lookup_is_case_insensitive() {
        assert_eq!(name("gb"), Some("United Kingdom"));
        assert_eq!(name(" JP "), Some("Japan"));
        assert_eq!(name("zz"), None);
    }

    #[test]
    fn code_lookup_accepts_aliases_folded_names_and_bare_codes() {
        assert_eq!(code("Korea, Republic of"), Some("KR"));
        assert_eq!(code("cote divoire"), Some("CI"));
        assert_eq!(code("COTE D'IVOIRE"), Some("CI"));
        assert_eq!(code("Côte d'Ivoire"), Some("CI"));
        assert_eq!(code("São Tomé and Príncipe"), Some("ST"));
        assert_eq!(code("Türkiye"), Some("TR"));
        assert_eq!(code("UK"), Some("GB"));
        assert_eq!(code("fi"), Some("FI"));
        assert_eq!(code("Atlantis"), None);
    }

    #[test]
    fn flag_uses_regional_indicators() {
        assert_eq!(flag("FI"), "🇫🇮");
        assert_eq!(flag("fi"), "🇫🇮");
        assert_eq!(flag("F"), "");
        assert_eq!(flag("F1"), "");
        assert_eq!(flag(""), "");
    }

    #[test]
    fn label_pairs_flag_with_name_or_falls_back_to_code() {
        assert_eq!(label("JP"), "🇯🇵 Japan");
        assert_eq!(label("jp"), "🇯🇵 Japan");
        assert_eq!(label("zz"), "ZZ");
    }
}
