//! The never-filled gate: labels of fields the program never fills, whatever the answer bank says.
//! Government identity numbers and banking, the owner's fixed rule. `AnswerBank::lookup` asks
//! `is_never_filled` before it reads any entry.

use std::sync::LazyLock;

use regex::Regex;

use crate::answers::contains_phrase;
use crate::text::{fold_look_alikes, normalize};

/// Terms that mark a field the program never fills, whatever words surround them: government
/// identity numbers and banking. Gonza's fixed rule, so it lives here and not in the editable
/// answers file: a loose bank pattern ("number", "country") must not be able to put a passport or
/// tax number, or an account, into a form that nobody reviewed.
///
/// This list holds only what is unambiguous on its own, matched like `CONSENT_NEVER` (normalized,
/// whole words, optional plural `s`), in English and Spanish. Bare "rfc", "sin", "ein", "pan" and
/// "run" are left out on purpose: they are ordinary words ("Experience writing RFCs", "sin
/// experiencia"). The rest of the rule is structural and lives in `is_never_filled`: a
/// number-word beside a document word (`DOCUMENT_WORDS`), a noun followed by a number-word for
/// accounts and cards, and a label that is only a generic word. Phrases stay specific: "LinkedIn
/// ID", "valid passport?", "Swift" the language or "banking" experience stay answerable.
const FIELD_NEVER: &[&str] = &[
    // Government identity numbers.
    "national id",
    "national identity",
    "national identification",
    "identity card number",
    "identity number",
    "identification number",
    "id number",
    "cedula",
    "rut",
    "numero de run",
    "dni",
    "cpf",
    "curp",
    "social security",
    "ssn",
    "social insurance number",
    "national insurance number",
    "passport number",
    "ni number",
    "ni no",
    "r u n",
    "swift bic",
    "vat id",
    "ifsc",
    "clabe",
    "tax code",
    "numero de pasaporte",
    "r u t",
    "government id",
    "government issued id",
    "identity document",
    "documento de identidad",
    "passport details",
    "passport id",
    "nif",
    "nie",
    "nin",
    "cuit",
    "cuil",
    "aadhaar",
    // Tax numbers.
    "tfn",
    "tax file number",
    "itin",
    "ein number",
    "abn",
    "pan number",
    "pan card",
    "utr number",
    "sin number",
    "rol unico nacional",
    "rol unico tributario",
    "carnet de identidad",
    "fiscal code",
    "codice fiscale",
    "identificacion fiscal",
    "ss number",
    "datos bancarios",
    "banking details",
    "account details",
    "tax id",
    "tax identification",
    "tax number",
    "tax nr",
    "tax reference number",
    "tax registration number",
    "taxpayer id",
    "taxpayer identification",
    "tax payer id",
    "tax payer identification",
    "tin number",
    "vat number",
    "numero de contribuyente",
    // Banking and cards.
    "bank account",
    "bank details",
    "beneficiary account",
    "cuenta corriente",
    "bsb",
    "aba number",
    "aba routing",
    "account number",
    "iban",
    "routing number",
    "sort code",
    "swift code",
    "bic code",
    "cuenta bancaria",
    "numero de cuenta",
    "credit card number",
    "debit card number",
    "card number",
    "cvv",
    "cvc",
    // Driver's licence numbers.
    "driver s license number",
    "drivers license number",
    "driver license number",
    "driver s licence number",
    "drivers licence number",
    "driver licence number",
    "license number",
    "licence number",
];

/// Words that say "a number" once a label is normalized. The abbreviations "No.", "Nr.", "Nro.",
/// "N.", "Nº", "N°" and "#" are turned into "number" before that (`read_label`), because a plain
/// "no" or "n" is also an answer ("If no, please explain", "(Y/N)") or "nor"; the one other place
/// where "no" is read as a number is the very end of the label (`ends_with_no_after_a_document`).
const NUMBER_WORDS: &[&str] = &["number", "nro", "num", "numero"];

/// Words that are not document words in general ("tax software") but are one when the label ends
/// in "No" right after them ("Tax No"): used only by `ends_with_no_after_a_document`.
const DOCUMENT_BEFORE_FINAL_NO: &[&str] = &["tax"];

/// Strong document terms: a real number-word ANYWHERE in the label with one of them holds ("Enter
/// the number exactly as it appears on your passport"). They name a government document and
/// nothing else.
const STRONG_DOCUMENT_TERMS: &[&str] = &[
    "passport",
    "pasaporte",
    "cedula",
    "national id",
    "identity card",
    "government id",
    "government issued id",
    "documento de identidad",
    "driver s licence",
    "driver s license",
    "drivers licence",
    "drivers license",
    "driving licence",
    "driving license",
    "social security",
];

/// Weak document words, which are also ordinary words ("tax software", "document the process"):
/// a number-word within `MAX_WORDS_APART` words of one of them, in either order, holds.
const DOCUMENT_WORDS: &[&str] = &[
    "id",
    "identity",
    "identidad",
    "identification",
    "identificacion",
    "document",
    "documento",
    "cedula",
    "carnet",
    "taxpayer",
    "tributario",
    "contribuyente",
    "licence",
    "license",
    "licencia",
    "national insurance",
    "visa grant",
    "medicare",
];

/// "Within three words of each other": the words between a number-word and a document word, plus
/// one. "Tax File Number (TFN)" is two apart, "Number of years of experience with identity
/// management" is five.
const MAX_WORDS_APART: usize = 3;

/// Words a label may be made of, besides document words, and still say only "the document".
const DOCUMENT_ONLY_FILLER: &[&str] = &[
    "your",
    "the",
    "a",
    "an",
    "or",
    "and",
    "please",
    "enter",
    "provide",
    "doc",
    "tu",
    "su",
    "el",
    "la",
    "de",
    "o",
    "y",
    "personal",
    "national",
    "government",
    "issued",
    "card",
    "document",
    "documento",
    "licence",
    "license",
    "licencia",
    "tax",
];

/// At least one of these must be in a label made of document words to hold it: "Tax" or
/// "Document" alone could be anything.
const DOCUMENT_ONLY_ANCHORS: &[&str] = &[
    "passport",
    "pasaporte",
    "cedula",
    "id",
    "identity",
    "identification",
    "identidad",
    "identificacion",
    "rut",
    "run",
    "dni",
    "cpf",
    "curp",
    "ssn",
    "nin",
    "nif",
    "nie",
    "cuit",
    "cuil",
    "tfn",
    "itin",
];

/// Nouns that are a banking field only when a number-word follows them: "account number",
/// "acct #", "card no". Order matters, so "Number of accounts managed" and "Card games" pass.
const NOUNS_THEN_NUMBER: &[&str] = &["account", "acct", "card"];

/// Spanish nouns that are a banking field when "number of" comes first: "número de cuenta",
/// "N° de tarjeta".
const NUMBER_THEN_DE_NOUNS: &[&str] = &["cuenta", "tarjeta"];

/// Labels that are only one of these words are held. A section such as "Passport" or "Bank" may
/// hold an input labelled just "Number": the scanner uses the group caption only for a field that
/// has no label (`field_scan.js`), so the lookup sees the bare word and cannot tell which number it
/// is. Holding it keeps a loose bank pattern from answering it.
const GENERIC_LABELS: &[&str] = &["number", "no", "numero", "nro", "num", "id", "n"];

/// "as on your national ID", "as shown on your passport", "in your identity card", "como aparece
/// en su cédula", "según tu pasaporte", on a normalized label: the document a name must match.
static NAMED_DOCUMENT: LazyLock<Regex> = LazyLock::new(|| {
    let prep = r"(?:on|in|per|from|according\s+to|must\s+match|matches|matching|match|tal\s+cual|igual\s+que|igual\s+a|en|segun|conforme\s+a|de\s+acuerdo\s+(?:a|con))";
    let article = r"(?:(?:your|the|my|su|tu|el|la|mi)\s+)?";
    let document = r"(?:government(?:\s+issued)?\s+id(?:\s+card)?|driver\s+s\s+licen[cs]e|drivers\s+licen[cs]e|national\s+id(?:\s+card)?|national\s+identity\s+(?:card|document)|identity\s+(?:card|document)|id\s+card|id|passport|dni|cedula(?:\s+de\s+identidad)?|pasaporte|documento(?:\s+de\s+identidad)?|document|carnet(?:\s+de\s+identidad)?)";
    // Several documents: "passport/ID" (the slash is a space once normalized), "passport or
    // national ID", "cédula o pasaporte".
    let more = format!(r"(?:\s+(?:(?:or|o|and|y)\s+)?{article}{document})*");
    Regex::new(&format!(r"\b{prep}\s+{article}{document}{more}\b"))
        .expect("NAMED_DOCUMENT is valid")
});

/// "Yes/No", "Y/N", "Sí/No", "yes or no", with an optional "/N/A": the answers a question lists,
/// not words of the label. Read on the raw label (accents still there).
static YES_NO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:yes|y|si|sí)\s*(?:/|or|o)\s*(?:no|n)\b(?:\s*/\s*(?:n\s*/\s*a|na)\b)?")
        .expect("YES_NO is a valid regex")
});

/// "pre-tax", "before tax", "after tax": says what a salary is counted on, never a tax ID. Taken
/// out of the label so that "salary (AUD, pre-tax)? Numbers only" does not read as "tax numbers".
static TAX_BASIS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:pre|post|before|after)[\s-]*tax(?:es)?\b").expect("TAX_BASIS is valid")
});

/// "e.g.", "ej:", "if applicable", "si aplica": a remark about the label, not words of it. Taken
/// out so that "Identification (e.g. passport)" is read as the documents it names.
static LABEL_ASIDE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:e\.?\s?g\.?|ej\.?|i\.e\.)\s*:?|\b(?:if\s+applicable|si\s+aplica|si\s+corresponde)\b")
        .expect("LABEL_ASIDE is valid")
});

/// "C.I." (cédula/carnet de identidad) with its dots, in any case. Only in the raw label: the
/// normalized "c i" would match far too much.
static CARNET_ABBREVIATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bc\.\s*i\.").expect("CARNET_ABBREVIATION is valid"));

/// Document and tax-ID tokens that are only the ID when written in capitals: "RUN", "NIT", "RFC"...
/// Lowercase "run" is a verb and "rfc" a standard ("Experience writing RFCs"), so this reads the
/// raw label and `read_label` never lowercases it first.
static CAPITAL_TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:RUN|NIT|RFC|TIN|CBU|CUIT|CUIL)\b").expect("CAPITAL_TOKENS is valid")
});

/// "Nº", "N°", "N˚": the ordinal sign makes the "N" an abbreviation of "number".
static ORDINAL_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bn[º°˚]").expect("ORDINAL_NUMBER is a valid regex"));

/// "No.", "Nr.", "Nro.", "Num.", "N." written with a dot. The character before is captured: a
/// letter or a dot in front ("R.U.N.", "Anno.") means the "n" is part of something else.
static DOTTED_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^|[^\p{L}.])(?:no|nr|nro|num|n)\.").expect("DOTTED_NUMBER is a valid regex")
});

/// The label as it is matched: the abbreviations of "number" written as "number", the answers a
/// question lists ("(Y/N)") removed, accents folded (`normalize` alone would turn "cédula" into
/// "c dula") and everything else as `normalize` reads it.
fn read_label(label: &str) -> String {
    let text = TAX_BASIS.replace_all(label, " ");
    let text = LABEL_ASIDE.replace_all(&text, " ");
    let text = CARNET_ABBREVIATION.replace_all(&text, " carnet de identidad ");
    let text = ORDINAL_NUMBER.replace_all(&text, " number ");
    let text = text.replace(['º', '°', '˚'], ".").replace('#', " number ");
    let text = YES_NO.replace_all(&text, " ");
    let text = DOTTED_NUMBER.replace_all(&text, "$1 number ");
    normalize(&fold_look_alikes(&text))
}

/// True when `label` is a field the program never fills (see `FIELD_NEVER` and `AnswerBank::lookup`).
pub fn is_never_filled(label: &str) -> bool {
    if CAPITAL_TOKENS.is_match(label) {
        return true;
    }
    let text = read_label(label);
    GENERIC_LABELS.contains(&text.trim()) || is_held_after_reading(&without_named_document(&text))
}

/// The rules of the never-filled list on a label already read (`read_label`) and, for a name
/// field, already freed of its document reference: a term that holds alone, a number-word beside
/// a document word, a banking noun with its number, "no" closing the label after a document word,
/// or a label made only of document words.
fn is_held_after_reading(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    FIELD_NEVER
        .iter()
        .any(|phrase| contains_phrase(text, &normalize(phrase)))
        || number_beside_document(&words)
        || number_beside_banking_noun(&words)
        || ends_with_no_after_a_document(&words)
        || spanish_no_de_a_document(&words)
        || only_document_words(&words)
}

/// "No de pasaporte" without the dot: Spanish "no" directly before "de" and a document word is
/// the abbreviation of "número".
fn spanish_no_de_a_document(words: &[&str]) -> bool {
    words.windows(3).any(|triple| {
        triple[0] == "no"
            && triple[1] == "de"
            && (DOCUMENT_WORDS.contains(&triple[2])
                || STRONG_DOCUMENT_TERMS.contains(&triple[2])
                || NUMBER_THEN_DE_NOUNS.contains(&triple[2]))
    })
}

/// True for a label made only of document words and filler ("Your ID", "Passport or ID", "ID
/// (passport or national)"), with at least one anchor among them: nothing in it says it is not the
/// document's number. "LinkedIn ID" and "Job ID" have a word of their own and are not.
fn only_document_words(words: &[&str]) -> bool {
    // "Pasaporte (opcional)" is the document; the marker is not a word of the label.
    let words: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !REQUIRED_MARKERS.contains(w))
        .collect();
    words
        .iter()
        .all(|w| DOCUMENT_ONLY_FILLER.contains(w) || DOCUMENT_ONLY_ANCHORS.contains(w))
        && words.iter().any(|w| DOCUMENT_ONLY_ANCHORS.contains(w))
}

/// Words a form adds after a label to say whether the field is required.
const REQUIRED_MARKERS: &[&str] = &[
    "required",
    "optional",
    "obligatorio",
    "opcional",
    "requerido",
];

/// "Passport No", "ID No", "Account no": a "no" or "n" that closes the label right after a
/// document or banking word is the abbreviation. Anywhere else it is a plain word.
fn ends_with_no_after_a_document(words: &[&str]) -> bool {
    // A "(required)" or "*" after the label moves the "no" off the end; only for this check the
    // marker words are dropped ("*" and brackets are already gone after `normalize`).
    let kept = words
        .iter()
        .rposition(|w| !REQUIRED_MARKERS.contains(w))
        .map_or(0, |last| last + 1);
    let words = &words[..kept];
    let [.., before, last] = words else {
        return false;
    };
    let ends_a_document_term = || {
        DOCUMENT_WORDS
            .iter()
            .chain(DOCUMENT_BEFORE_FINAL_NO)
            .chain(STRONG_DOCUMENT_TERMS)
            .any(|term| term.rsplit(' ').next() == Some(*before))
    };
    ["no", "n", "nr"].contains(last)
        && (NOUNS_THEN_NUMBER.contains(before) || ends_a_document_term())
}

/// "Full name (as on your national ID)" asks for a name that matches a document, not for the
/// document's number: the reference to the document is taken out of a label that asks for a name.
/// Only when what is left has neither a number-word nor a document word: "Name and number as on
/// your national ID" and "Name and ID as on your national ID" are kept whole.
fn without_named_document(text: &str) -> String {
    let asks_for_a_name = ["name", "nombre", "surname", "apellido"]
        .iter()
        .any(|word| contains_phrase(text, word));
    if !asks_for_a_name {
        return text.to_string();
    }
    let rest = NAMED_DOCUMENT.replace_all(text, " ").into_owned();
    let says_more = |w: &str| NUMBER_WORDS.contains(&w) || DOCUMENT_ONLY_ANCHORS.contains(&w);
    if rest.split_whitespace().any(says_more) {
        text.to_string()
    } else {
        rest
    }
}

/// True when a number-word and a document word are at most `MAX_WORDS_APART` words apart.
fn number_beside_document(words: &[&str]) -> bool {
    let numbers: Vec<usize> = (0..words.len())
        .filter(|&i| NUMBER_WORDS.contains(&words[i]))
        .collect();
    if numbers.is_empty() {
        return false;
    }
    let has_strong = STRONG_DOCUMENT_TERMS.iter().any(|term| {
        let term: Vec<&str> = term.split(' ').collect();
        words.windows(term.len()).any(|window| window == term)
    });
    has_strong
        || DOCUMENT_WORDS.iter().any(|term| {
            let term: Vec<&str> = term.split(' ').collect();
            words
                .windows(term.len())
                .enumerate()
                .filter(|(_, window)| *window == term.as_slice())
                .any(|(start, _)| {
                    let end = start + term.len() - 1;
                    numbers.iter().any(|&n| {
                        let apart = if n < start {
                            start - n
                        } else {
                            n.saturating_sub(end)
                        };
                        apart <= MAX_WORDS_APART
                    })
                })
        })
}

/// True for "account number", "acct #", "card no" (noun, then number-word) and the Spanish
/// "número de cuenta", "N° de tarjeta" (number-word, "de", noun).
fn number_beside_banking_noun(words: &[&str]) -> bool {
    words
        .windows(2)
        .any(|pair| NOUNS_THEN_NUMBER.contains(&pair[0]) && NUMBER_WORDS.contains(&pair[1]))
        || words.windows(3).any(|triple| {
            NUMBER_WORDS.contains(&triple[0])
                && triple[1] == "de"
                && NUMBER_THEN_DE_NOUNS.contains(&triple[2])
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answers::{AnswerBank, AnswerEntry};

    /// Labels of the fields the program never fills: identity numbers and banking.
    const NEVER_FILLED_LABELS: &[&str] = &[
        "National ID",
        "National identity number",
        "Identity card number",
        "ID number",
        "ID #",
        "Cédula de identidad",
        "Cedula",
        "RUT",
        "Número de RUN",
        "DNI",
        "CPF",
        "CURP",
        "Social security number",
        "SSN",
        "Social insurance number",
        "National insurance number",
        "Passport number",
        "Passport no.",
        "Passport #",
        "Número de pasaporte",
        "Tax ID",
        "Tax identification number",
        "TIN number",
        "Taxpayer ID",
        "Taxpayer identification number",
        "Tax payer ID",
        "VAT number",
        "Número de contribuyente",
        "Bank account",
        "Account number",
        "IBAN",
        "Routing number",
        "Sort code",
        "SWIFT code",
        "BIC code",
        "Cuenta bancaria",
        "Número de cuenta",
        "Credit card number",
        "Debit card number",
        "CVV",
        "Driver's license number",
        "Driver’s licence number",
        // A number-word beside a document word, in either order.
        "Tax number",
        "Tax File Number (TFN)",
        "National Insurance (NI) number",
        "NI number",
        "ID document number",
        "Identity document number",
        "Document number",
        "Número de documento",
        "Número de identificación",
        "Nº de pasaporte",
        "N° de pasaporte",
        "Nro. de pasaporte",
        "Número pasaporte",
        "Pasaporte (número)",
        "Visa grant number",
        "Medicare number",
        "ID No.",
        // Order-sensitive: the noun first, or the Spanish "number of account".
        "Account no.",
        "Passport Nr.",
        "ID Nr.",
        "Tax Nr.",
        "Passport #:",
        "Passport num",
        "N˚ de pasaporte",
        "N° RUT",
        "NI no",
        "NI number",
        "Acct #",
        "N° de cuenta",
        "Nro. de cuenta",
        "Card number",
        "Card no.",
        "Número de tarjeta",
        // Terms that hold alone.
        "R.U.T.",
        "TFN",
        "Tax File Number",
        "ITIN",
        "EIN number",
        "ABN",
        "BSB",
        "Aadhaar",
        "PAN number",
        "PAN card",
        "NIN",
        "NIF",
        "NIE",
        "CUIT",
        "CUIL",
        "UTR number",
        "ABA number",
        "ABA routing",
        "SIN number",
        "Government ID",
        "Government issued ID",
        "Identity document",
        "Documento de identidad",
        "Passport details",
        "Passport ID",
        "Bank details",
        "Cuenta corriente",
        "Beneficiary account",
        // A name label that also asks for a number is not only a name.
        "Name and number as on your national ID",
        "Name and document number per your national ID card",
        "Nombre y número de cédula",
        "Name as on your passport, and passport number",
        "Name and passport number as on passport",
        "ID number as it appears on your government ID",
        "Name and ID as on your national ID",
        "Nombre y RUN según cédula",
        // A strong document word with a number-word anywhere in the label.
        "Enter the number exactly as it appears on your passport",
        "Number shown on the front of your passport",
        // More terms that hold alone.
        "R.U.N.",
        "SWIFT BIC",
        "VAT ID",
        "IFSC",
        "CLABE",
        "Tax code",
        // Tax IDs held by phrase, now that the bare word "tax" is not a document word.
        "Tax identification (TIN/SSN)",
        "Tax Nr",
        "Tax reference number",
        "Tax registration number",
    ];

    /// Terms that hold alone by phrase (any case), in Chilean, foreign and banking wording.
    const MORE_TERMS_THAT_HOLD_ALONE: &[&str] = &[
        "Rol Único Nacional / Tributario",
        "Número de carnet",
        "Carnet de identidad",
        "N° de carnet de identidad",
        "No de pasaporte",
        "Doc. identidad",
        "Doc. de identidad",
        "Pasaporte (opcional)",
        "Passport (optional)",
        "Passport (if applicable)",
        "Identification (e.g. passport)",
        "Fiscal code",
        "Codice fiscale",
        "Identificación fiscal",
        "SS#",
        "Datos bancarios",
        "Banking details",
        "Account details",
    ];

    /// Held through a token written in capitals or with dots in the RAW label ("RUN", "C.I."):
    /// lowercase "run" is a verb, so these are tried as written, not lowercased.
    const RAW_TOKEN_LABELS: &[&str] = &[
        "RUN (sin puntos ni guión)",
        "RUN (ej: 12345678-9)",
        "Ingrese su RUN",
        "RUN del postulante",
        "C.I.",
        "c.i.",
        "C.I. N°",
        "NIT",
        "RFC",
        "TIN",
        "CBU",
        "CUIT",
        "CUIL",
    ];

    /// Held on purpose although they could be read as ordinary: holding is the safe side. "Job ID
    /// number" and "Employee ID number" hold through "id number", "bank accounts" through "bank
    /// account".
    const HELD_ON_PURPOSE: &[&str] = &[
        "Job ID number",
        "Employee ID number",
        "Experience reconciling bank accounts",
    ];

    /// A label made only of document words: nothing else says what it asks for. Tried plain, in
    /// both cases and after "Please enter your" (a "(required)" would add a word).
    const ONLY_DOCUMENT_WORDS: &[&str] = &[
        "Passport",
        "Pasaporte",
        "Your ID",
        "Personal ID",
        "Passport or ID",
        "ID (passport or national)",
        "Identification *",
        "Identity card",
        "ID card",
        "Document ID",
        "ID document",
        "RUT",
        "RUN",
        "R.U.N.",
        "DNI",
    ];

    /// Labels that are only a generic word: a section caption such as "Passport" may be missing
    /// from the label the scanner reports, which is then just this word.
    const GENERIC_LABELS: &[&str] = &[
        "Number", "No", "Numero", "Número", "Nro", "Nro.", "Num", "ID", "N", "Number *", "No.",
    ];

    /// Labels that only look like the ones above: none of them may be held back.
    const ORDINARY_LABELS: &[&str] = &[
        "LinkedIn ID",
        "Requisition ID",
        "Job ID",
        "Do you have a valid passport?",
        "Years of experience with Swift",
        "Swift",
        "How long did you run the project",
        "Have you run a team?",
        "Sin experiencia",
        "Are you comfortable with a contractor arrangement where you handle your own taxes?",
        "Accounting experience",
        "Account manager",
        "GitHub account",
        "Experience in banking",
        "Number of years",
        "Phone number",
        "What drives you?",
        "LinkedIn profile or ID",
        "LinkedIn ID / URL",
        "Employee referral: name and ID",
        // A number-word with no document beside it, or a document word with no number.
        "Number of years of experience with identity management platforms",
        "Number of direct reports",
        "What is your tax residency country?",
        "Country of tax residence",
        "Number of accounts managed",
        "Account manager experience",
        "Card games you like",
        "Experience writing RFCs",
        "sin experiencia",
        // A salary field with a tax hint, and tax talk with no tax ID in it.
        "What are your salary expectations (AUD, pre-tax)? Numbers only",
        "Expected salary before tax (number)",
        "What is your expected annual base salary (pre-tax)? Please enter numbers only.",
        "Describe your experience with tax software, No. of years",
        "Number of tax returns you have prepared",
        "Expected salary (USD, numbers only)",
        // Number-words and ID words that are not a document number.
        "No. of years",
        "Years of experience with ID systems",
        "Identity and access management: number of years",
        "Mobile no.",
        "Nº de teléfono",
        "Skype ID",
        "Employee referral ID",
        "Licencia de conducir (S/N)",
        "¿Tienes carnet de conducir? (Sí/No)",
        "How do you run tests?",
        "Can you run on-call?",
        "Experience writing RFCs",
        "Banco",
        "Bank name",
        "Tipo de cuenta",
        "Account holder name",
        "Passport expiry date",
        "Passport country",
        "Are you a U.S. taxpayer?",
        "Do you have a valid passport? (Yes/No)",
        "Do you have a valid passport? Yes or No",
    ];

    /// Name fields that only say which document the name must match. Each one is held WITHOUT the
    /// name exemption (it holds a term that holds alone), so it proves the exemption.
    const NAME_EXEMPTION_PROOF: &[&str] = &[
        "Full Name (as on your national ID)",
        "Please enter your legal name exactly as shown on your government-issued ID.",
        "Name as on your government ID",
        "Nombre completo (como aparece en su cédula de identidad)",
        "Legal full name (must match your government-issued ID)",
        "Full name, matching your national ID",
        "Nombre completo (tal cual tu cédula)",
        "Surname (as in your national ID)",
        "Full name (as shown on your passport/ID)",
        "Full legal name (as it appears on your passport or national ID)",
        "Nombre completo (como aparece en tu cédula o pasaporte)",
        "Name as on your national identity document",
        "Apellidos según documento de identidad",
    ];

    /// Name fields answerable with or without the exemption: they hold no term that holds alone
    /// and no number-word, so they say nothing about the exemption.
    const NAME_ANSWERABLE_EITHER_WAY: &[&str] = &[
        "Legal name as shown on your passport",
        "Name as in your identity card",
        "Nombre según tu pasaporte",
        "Nombre tal como figura en el documento",
        "Full name as it appears on your passport",
        "Name as shown on your driver's licence",
        "Name as on your passport (no nicknames)",
        "Name (as on ID/passport)",
    ];

    /// "no" and "n" are plain words here (an answer, a negation, "ni" is "nor"): none of them is
    /// the abbreviation of "number", so none of these may be held.
    const PLAIN_SHORT_WORDS: &[&str] = &[
        "Do you have a valid passport? (Y/N)",
        "Do you have a valid driver's license? Y/N",
        "Do you hold a valid driver's license? If no, please explain",
        "Do you have experience with tax software? No experience is fine",
        "Do you have a government-issued photo ID? (Y/N)",
        "Are you a tax resident? Y/N",
        "¿Tienes pasaporte vigente? (Sí/No)",
        "Do you have a valid passport? (Yes/No/N/A)",
        "What's your approach to license compliance? No more than 100 words",
        "Identidad de género (no obligatorio)",
        "Pretensión de renta (no incluyas puntos ni comas)",
        "Pretensión de renta en CLP, no uses puntos ni comas",
        "Teléfono (no uses ni espacios ni guiones)",
        "Do you live in NI? (Y/N)",
    ];

    /// The abbreviation sits at the very end after a document or banking word, so these are only
    /// tried plain and after "Please enter your" (a trailing "(required)" would move it).
    const ENDS_WITH_NO: &[&str] = &[
        "Passport No",
        "ID No",
        "Licence no",
        "Account no",
        "Card no",
        "Tax no",
    ];

    fn loose_and_exact_bank(label: &str) -> AnswerBank {
        // The loose entry is made of the label's own words, so it matches the label for sure.
        let loose = normalize(label).replace(' ', "|");
        let exact = format!("={label}");
        AnswerBank {
            entries: vec![
                AnswerEntry::new(&[exact.as_str()], Some("exact")),
                AnswerEntry::new(&[loose.as_str()], Some("loose")),
            ],
            consent_phrases: vec![],
        }
    }

    #[test]
    fn no_bank_entry_answers_an_identity_number_or_a_bank_field() {
        for label in NEVER_FILLED_LABELS {
            for shown in [
                label.to_string(),
                label.to_uppercase(),
                label.to_lowercase(),
                format!("Please enter your {label} *"),
                format!("{label} (required)"),
            ] {
                let bank = loose_and_exact_bank(&shown);
                assert!(
                    bank.lookup(&shown).is_none(),
                    "a bank entry answered {shown:?}"
                );
            }
        }
    }

    #[test]
    fn terms_that_hold_alone_hold_in_any_case_and_with_a_marker() {
        for label in MORE_TERMS_THAT_HOLD_ALONE {
            for shown in [
                label.to_string(),
                label.to_uppercase(),
                label.to_lowercase(),
                format!("Please enter your {label} *"),
                format!("{label} (required)"),
            ] {
                let bank = loose_and_exact_bank(&shown);
                assert!(
                    bank.lookup(&shown).is_none(),
                    "a bank entry answered {shown:?}"
                );
            }
        }
    }

    #[test]
    fn a_token_in_capitals_or_with_dots_holds_as_written() {
        for label in RAW_TOKEN_LABELS {
            for shown in [
                label.to_string(),
                format!("Please enter your {label} *"),
                format!("{label} (required)"),
            ] {
                let bank = loose_and_exact_bank(&shown);
                assert!(
                    bank.lookup(&shown).is_none(),
                    "a bank entry answered {shown:?}"
                );
            }
        }
    }

    #[test]
    fn a_held_on_purpose_label_is_held() {
        for label in HELD_ON_PURPOSE {
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_none(), "answered {label:?}");
        }
    }

    #[test]
    fn a_label_made_only_of_document_words_is_held() {
        for label in ONLY_DOCUMENT_WORDS {
            for shown in [
                label.to_string(),
                label.to_uppercase(),
                label.to_lowercase(),
                format!("Please enter your {label} *"),
            ] {
                let bank = loose_and_exact_bank(&shown);
                assert!(
                    bank.lookup(&shown).is_none(),
                    "a bank entry answered {shown:?}"
                );
            }
        }
    }

    #[test]
    fn the_name_exemption_is_what_answers_these_name_fields() {
        for label in NAME_EXEMPTION_PROOF {
            // Without the exemption the label is held: the proof that the exemption does the work.
            assert!(
                is_held_after_reading(&read_label(label)),
                "{label:?} would be answerable without the exemption: it proves nothing"
            );
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_some(), "{label:?} was held back");
        }
        for label in NAME_ANSWERABLE_EITHER_WAY {
            assert!(
                !is_held_after_reading(&read_label(label)),
                "{label:?} belongs with the proofs: it is held without the exemption"
            );
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_some(), "{label:?} was held back");
        }
    }

    #[test]
    fn a_required_or_optional_marker_after_the_label_does_not_hide_the_final_no() {
        for label in [
            "Passport No (required)",
            "Account no *",
            "Card no (optional)",
            "Licence no (obligatorio)",
            "ID No (opcional)",
            "Passport No - required",
            "ID No: REQUERIDO",
        ] {
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_none(), "answered {label:?}");
        }
        let phone = "Phone no (required)";
        assert!(loose_and_exact_bank(phone).lookup(phone).is_some());
    }

    #[test]
    fn a_no_at_the_end_after_a_document_word_is_a_number() {
        for label in ENDS_WITH_NO {
            for shown in [
                label.to_string(),
                label.to_uppercase(),
                format!("Please enter your {label} *"),
            ] {
                let bank = loose_and_exact_bank(&shown);
                assert!(
                    bank.lookup(&shown).is_none(),
                    "a bank entry answered {shown:?}"
                );
            }
        }
    }

    #[test]
    fn a_plain_no_or_n_is_not_the_abbreviation_of_number() {
        for label in PLAIN_SHORT_WORDS {
            let bank = loose_and_exact_bank(label);
            assert!(
                bank.lookup(label).is_some(),
                "{label:?} was held back by a plain \"no\" or \"n\""
            );
        }
    }

    #[test]
    fn a_label_that_is_only_a_generic_word_is_held_too() {
        for label in GENERIC_LABELS {
            let bank = loose_and_exact_bank(label);
            assert!(
                bank.lookup(label).is_none(),
                "a bank entry answered the bare label {label:?}"
            );
        }
        // Inside a longer label the same word says nothing by itself.
        assert!(
            loose_and_exact_bank("Phone number")
                .lookup("Phone number")
                .is_some()
        );
    }

    #[test]
    fn a_bank_still_answers_a_label_that_only_looks_like_a_never_filled_one() {
        for label in ORDINARY_LABELS {
            let bank = loose_and_exact_bank(label);
            assert!(
                bank.lookup(label).is_some(),
                "{label:?} was held back by the never-filled list"
            );
        }
    }

    #[test]
    fn a_catch_all_pattern_does_not_fill_a_passport_number() {
        let bank = AnswerBank {
            entries: vec![AnswerEntry::new(&["number|country"], Some("1234"))],
            consent_phrases: vec![],
        };
        assert!(bank.lookup("Passport number").is_none());
        assert!(bank.lookup("Phone number").is_some());
    }
}
