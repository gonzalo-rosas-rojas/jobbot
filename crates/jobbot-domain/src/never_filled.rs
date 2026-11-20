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
/// This list has only what is unambiguous on its own, matched like `CONSENT_NEVER` (normalized,
/// whole words, optional plural `s`), in English and Spanish. Short words that are also ordinary
/// ones are not here in lowercase ("sin experiencia", "Experience writing RFCs").
///
/// What is caught in capitals, on the raw label (`capital_token_is_an_id`): RUN, NIT, RFC, TIN,
/// CBU, CUIT and CUIL. In a label with lowercase letters any of them counts. In one written
/// entirely in capitals a token counts anywhere in up to three words, or when it starts the label
/// or follows a word of `WORDS_BEFORE_AN_ID_TOKEN`; RFC also needs a label of at most four words.
/// EIN, PAN and SIN are NOT caught in capitals (ordinary words and acronyms in English): only
/// their phrases below ("ein number", "pan card", "sin number") count. "C.I." is read by
/// `carnet_abbreviation_is_an_id`.
///
/// The rest of the rule is structural and read by `is_never_filled_after_reading`: a number-word
/// beside a document of `DOCUMENTS`, a banking noun with its number, a label of only document
/// words, a bare generic word. Phrases stay specific so that "LinkedIn ID", "valid passport?", the
/// Swift language or experience in banking stay answerable.
const NEVER_FILLED_TERMS: &[&str] = &[
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
    "tax id",
    "tax identification",
    "taxpayer id",
    "taxpayer identification",
    "tax payer id",
    "tax payer identification",
    "tin number",
    "vat number",
    "numero de contribuyente",
    "registro federal de contribuyentes",
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
/// "N.", "Nº", "N°" and "#" are turned into "number" before that (`read_label`) and a plain "no" or
/// "n" is deliberately not here: it is also an answer ("If no, please explain", "(Y/N)") or "nor".
/// The one other place where "no" is read as a number is the very end of the label
/// (`ends_with_no_after_a_document`). The plural is a number-word too ("Document numbers"); a
/// salary field's "numbers only" is kept out by `TAX_BASIS`, not by dropping the plural.
const NUMBER_WORDS: &[&str] = &["number", "numbers", "nro", "num", "numero"];

/// The words that may stand between "tax" and its number-word: "Tax payer number", "Tax residence
/// number", "Tax Reg. No.", "Tax reference No". One word at most, so "tax software, No. of years"
/// is not a tax ID.
const TAX_QUALIFIERS: &[&str] = &[
    "payer",
    "identifier",
    "identification",
    "id",
    "residence",
    "residency",
    "reg",
    "registration",
    "ref",
    "reference",
    "file",
];

/// "Tax payer registration number", "Tax registration ref no.": two qualifiers at most.
const MAX_TAX_QUALIFIERS: usize = 2;

/// How much a document term says on its own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Names a government document and nothing else: a number-word ANYWHERE in the label makes it
    /// never filled ("Enter the number exactly as it appears on your passport").
    Strong,
    /// Also an ordinary word ("document the process", "identity management"): never filled only
    /// with a number-word within `MAX_WORDS_APART` words.
    Weak,
    /// An acronym or code, an anchor of a documents-only label and, for "dni", of the name
    /// exemption. It is never filled alone only through `NEVER_FILLED_TERMS` (dni, rut, cpf, curp,
    /// ssn, nin, nif, nie, cuit, cuil, tfn, itin) or, for RUN, in capitals
    /// (`capital_token_is_an_id`). The rows of this table do not add EIN, PAN or SIN.
    Code,
    /// A document phrase that only the name exemption needs ("id card").
    NameOnly,
}

/// One row of the documents table. Adding a document is one line in `DOCUMENTS`.
struct Document {
    /// Normalized words (no accents or punctuation), separated by single spaces.
    term: &'static str,
    kind: Kind,
    /// Counts as "the document" in a label made only of document words ("Your ID") and in a name
    /// label that still says a document after the exemption ("Name and ID as on your national ID").
    anchor: bool,
    /// May also appear in a documents-only label without being an anchor ("Document ID").
    companion: bool,
    /// Is read as the document a name must match ("as on your <term>").
    named: bool,
}

impl Document {
    const fn new(term: &'static str, kind: Kind) -> Self {
        Document {
            term,
            kind,
            anchor: false,
            companion: false,
            named: false,
        }
    }
    const fn anchor(mut self) -> Self {
        self.anchor = true;
        self
    }
    const fn companion(mut self) -> Self {
        self.companion = true;
        self
    }
    const fn named(mut self) -> Self {
        self.named = true;
        self
    }
}

const fn strong(term: &'static str) -> Document {
    Document::new(term, Kind::Strong)
}
const fn weak(term: &'static str) -> Document {
    Document::new(term, Kind::Weak)
}
const fn code(term: &'static str) -> Document {
    Document::new(term, Kind::Code).anchor()
}
const fn name_only(term: &'static str) -> Document {
    Document::new(term, Kind::NameOnly).named()
}

/// Every document the gate knows. The strong list, the weak list, the anchors and the
/// name-exemption pattern are all read from here.
const DOCUMENTS: &[Document] = &[
    strong("passport").anchor().named(),
    strong("pasaporte").anchor().named(),
    strong("cedula").anchor().named(),
    strong("national id").named(),
    strong("identity card").named(),
    strong("government id").named(),
    strong("government issued id").named(),
    strong("documento de identidad").named(),
    strong("driver s licence").named(),
    strong("driver s license").named(),
    strong("drivers licence").named(),
    strong("drivers license").named(),
    strong("driving licence"),
    strong("driving license"),
    strong("social security"),
    name_only("cedula de identidad"),
    name_only("national id card"),
    name_only("national identity card"),
    name_only("national identity document"),
    name_only("identity document"),
    name_only("id card"),
    name_only("government id card"),
    name_only("government issued id card"),
    name_only("carnet de identidad"),
    weak("id").anchor().named(),
    weak("identity").anchor(),
    weak("identidad").anchor(),
    weak("identification").anchor(),
    weak("identificacion").anchor(),
    weak("document").companion().named(),
    weak("documento").companion().named(),
    weak("carnet").named(),
    weak("taxpayer"),
    weak("tributario"),
    weak("contribuyente"),
    weak("licence").companion(),
    weak("license").companion(),
    weak("licencia").companion(),
    weak("national insurance"),
    weak("visa grant"),
    weak("medicare"),
    code("dni").named(),
    code("rut"),
    code("run"),
    code("cpf"),
    code("curp"),
    code("ssn"),
    code("nin"),
    code("nif"),
    code("nie"),
    code("cuit"),
    code("cuil"),
    code("tfn"),
    code("itin"),
];

/// Words of a documents-only label that are not documents: the glue of "your ID or passport", the
/// asking verbs, and words that go before "ID" or "card" ("personal ID", "national", "tax").
const DOCUMENT_GLUE_WORDS: &[&str] = &[
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
    "tax",
];

fn terms_of(kind: Kind) -> impl Iterator<Item = &'static str> {
    DOCUMENTS
        .iter()
        .filter(move |document| document.kind == kind)
        .map(|document| document.term)
}

/// Whether `word` is an anchor: the document itself, so a label of only such words and glue words
/// asks for it.
fn is_anchor(word: &str) -> bool {
    DOCUMENTS
        .iter()
        .any(|document| document.anchor && document.term == word)
}

fn is_document_glue(word: &str) -> bool {
    DOCUMENT_GLUE_WORDS.contains(&word)
        || DOCUMENTS
            .iter()
            .any(|document| document.companion && document.term == word)
}

/// Whether `word` is a strong or weak document word (not a code).
fn is_document_word(word: &str) -> bool {
    terms_of(Kind::Strong)
        .chain(terms_of(Kind::Weak))
        .any(|term| term == word)
}

/// "Within three words of each other": the words between a number-word and a document word, plus
/// one. "Tax File Number (TFN)" is two apart, "Number of years of experience with identity
/// management" is five.
const MAX_WORDS_APART: usize = 3;

/// Nouns that are a banking field only when a number-word follows them: "account number",
/// "acct #", "card no". Order matters, so "Number of accounts managed" and "Card games" pass.
const NOUNS_THEN_NUMBER: &[&str] = &["account", "acct", "card"];

/// Spanish nouns that are a banking field when "number of" comes first: "número de cuenta",
/// "N° de tarjeta".
const NUMBER_THEN_DE_NOUNS: &[&str] = &["cuenta", "tarjeta"];

/// Labels that are only one of these words are never filled. A section such as "Passport" or
/// "Bank" may hold an input labelled just "Number": the scanner falls back to the group caption
/// only for a field that has no label at all (`field_scan.js`), so the lookup sees the bare word
/// and cannot tell which number it is. A loose bank pattern must not answer it.
const BARE_WORD_LABELS: &[&str] = &["number", "no", "numero", "nro", "num", "id", "n"];

/// Words that may wrap "account details" without changing what it is: "Please enter your account
/// details".
const ACCOUNT_DETAILS_WRAPPERS: &[&str] = &["please", "enter", "your"];

/// Words that make "account details" a bank account wherever it stands in the label.
const BANK_CONTEXT_WORDS: &[&str] = &[
    "payment", "salary", "payroll", "deposit", "bank", "banking", "transfer", "wire", "pago",
];

/// What a name field may say about the document its name must match: "as on your national ID",
/// "según tu pasaporte", "tal cual tu cédula", on a normalized label. Built from the `named` rows
/// of `DOCUMENTS`, longest first so that "national id card" is read before "national id".
static NAMED_DOCUMENT: LazyLock<Regex> = LazyLock::new(|| {
    let prep = r"(?:on|in|per|from|according\s+to|must\s+match|matches|matching|match|tal\s+cual|igual\s+que|igual\s+a|en|segun|conforme\s+a|de\s+acuerdo\s+(?:a|con))";
    let article = r"(?:(?:your|the|my|su|tu|el|la|mi)\s+)?";
    let mut terms: Vec<&str> = DOCUMENTS
        .iter()
        .filter(|document| document.named)
        .map(|document| document.term)
        .collect();
    terms.sort_by_key(|term| std::cmp::Reverse(term.len()));
    let document = format!(
        "(?:{})",
        terms
            .iter()
            .map(|term| term.replace(' ', r"\s+"))
            .collect::<Vec<_>>()
            .join("|")
    );
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

/// "pre-tax", "before tax", "excl. tax", "net of tax", "including tax": says what a salary or rate
/// is counted on, never a tax ID. Taken out of the label so that "salary (AUD, pre-tax)? Numbers
/// only" does not read as "tax numbers". It only strips words that cannot ask for a number.
///
/// Capture 1 is the word after "tax". When it is one that asks for a tax ID (`TAX_ID_WORDS`),
/// nothing is stripped: "Full name, including tax ID" is not a salary.
static TAX_BASIS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:pre|post|before|after|incl|including|inclusive\s+of|excl|excluding|exclusive\s+of|net\s+of|gross\s+of)[\s.-]*tax(?:es)?\b(?:\s+([\p{L}#]+))?",
    )
    .expect("TAX_BASIS is valid")
});

/// The words after "tax" that make a basis wording a real tax ID (the `TAX_QUALIFIERS` count too).
const TAX_ID_WORDS: &[&str] = &[
    "id",
    "number",
    "numbers",
    "no",
    "nr",
    "#",
    "code",
    "file",
    "identification",
    "reference",
    "registration",
    "payer",
];

/// "numbers only", "only numbers", "no numbers", "without numbers", "solo números": a format hint
/// after a punctuation mark, never a number-word of the label. Not stripped without the
/// punctuation, so "Account numbers only" still asks for a number.
static NUMBER_HINT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)[(\[,;:?.\-–—]\s*(?:numbers?\s+only|only\s+numbers?|no\s+numbers?|without\s+numbers?|(?:solo|sólo|sin)\s+números?)\b",
    )
    .expect("NUMBER_HINT is valid")
});

/// "e.g.", "ej:", "if applicable", "si aplica": a remark about the label, not words of it. Taken
/// out so that "Identification (e.g. passport)" is read as the documents it names.
static LABEL_ASIDE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:e\.?\s?g\.?|ej\.?|i\.e\.)\s*:?|\b(?:if\s+applicable|si\s+aplica|si\s+corresponde)\b")
        .expect("LABEL_ASIDE is valid")
});

/// "C.I." (cédula/carnet de identidad), in any case, with its dots, not glued to a letter or a dot
/// before it ("U.S.C.I.S."). Capture 1 is the character before, which the regex crate cannot look
/// behind for.
static CARNET_ABBREVIATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^|[^\p{L}.])c\.\s*i(?:\.|\s*:)").expect("CARNET_ABBREVIATION is valid")
});

/// Words around "C.I." that make it continuous integration, not the ID card.
const CONTINUOUS_INTEGRATION_WORDS: &[&str] = &[
    "pipeline",
    "pipelines",
    "cd",
    "build",
    "builds",
    "tool",
    "tools",
    "tooling",
    "server",
    "workflow",
    "workflows",
    "experience",
    "jenkins",
    "github",
    "gitlab",
    "deploy",
    "deploys",
    "deployment",
    "deployments",
    "deploying",
    "test",
    "tests",
    "testing",
];

/// Document and tax-ID tokens written in capitals: "RUN", "NIT", "RFC", "TIN", "CBU", "CUIT",
/// "CUIL". Lowercase they are ordinary words ("run" the verb, "rfc" the standard, "tin" the
/// metal); the sin, ein, pan and run of `NEVER_FILLED_TERMS` are only caught with the context
/// they have there. See `capital_token_is_an_id` for when a capital token counts.
static CAPITAL_TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:RUN|NIT|RFC|TIN|CBU|CUIT|CUIL)\b").expect("CAPITAL_TOKENS is valid")
});

/// What may stand right before an ID token for a label written entirely in capitals: forms that
/// style their labels in uppercase ("HOW DO YOU RUN TESTS?") would otherwise read as an ID.
const WORDS_BEFORE_AN_ID_TOKEN: &[&str] = &[
    "your", "su", "tu", "ingrese", "ingresa", "indique", "digite", "escriba", "enter", "del", "de",
    "numero", "número", "number", "n", "nº", "no", "nr", "nro", "num",
];

/// A label written in capitals of at most this many words is read for its token wherever it is:
/// "CHILEAN RUN", "VALID TIN". Longer ones are sentences ("HOW DO YOU RUN TESTS?").
const MAX_WORDS_WHERE_A_CAPITAL_TOKEN_COUNTS_ANYWHERE: usize = 3;

/// "RFC" is the tax ID only in a short label; in a long one it is the engineering document.
const MAX_WORDS_WITH_A_TAX_RFC: usize = 4;

/// The last word of `before`, lowercased: the word an ID token follows.
fn word_before(before: &str) -> Option<String> {
    before
        .split(|c: char| !c.is_alphanumeric())
        .rfind(|word| !word.is_empty())
        .map(str::to_lowercase)
}

/// Whether a capital token of `CAPITAL_TOKENS` in the raw label asks for an ID. In a label with
/// lowercase letters it always does, and so it does in a label in capitals of at most
/// `MAX_WORDS_WHERE_A_CAPITAL_TOKEN_COUNTS_ANYWHERE` words. In a longer one in capitals it does
/// only when it starts the label or follows one of `WORDS_BEFORE_AN_ID_TOKEN`. "RFC" needs a
/// label of at most `MAX_WORDS_WITH_A_TAX_RFC` words.
fn capital_token_is_an_id(label: &str) -> bool {
    let all_capitals = !label.chars().any(char::is_lowercase);
    let word_count = label
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .count();
    CAPITAL_TOKENS.find_iter(label).any(|token| {
        if token.as_str() == "RFC" && word_count > MAX_WORDS_WITH_A_TAX_RFC {
            return false;
        }
        if !all_capitals || word_count <= MAX_WORDS_WHERE_A_CAPITAL_TOKEN_COUNTS_ANYWHERE {
            return true;
        }
        word_before(&label[..token.start()])
            .is_none_or(|word| WORDS_BEFORE_AN_ID_TOKEN.contains(&word.as_str()))
    })
}

/// A "/" that opens another abbreviation ("C.I./C.D."), not a word ("C.I./Pasaporte").
fn glued_to_another_abbreviation(after: &str) -> bool {
    let Some(rest) = after.strip_prefix('/') else {
        return after.starts_with(char::is_alphabetic);
    };
    let mut chars = rest.trim_start().chars();
    chars.next().is_some_and(char::is_alphabetic) && chars.next() == Some('.')
}

/// Whether the label talks about continuous integration ("C.I. pipelines you have built",
/// "Experience with C.I. tools").
fn has_continuous_integration_context(label: &str) -> bool {
    label
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| CONTINUOUS_INTEGRATION_WORDS.contains(&word))
}

/// Whether "C.I." (or "C.I:") in the raw label is the Chilean ID card. A standalone one is,
/// unless it is glued to another abbreviation ("C.I./C.D.") or the label talks about continuous
/// integration. "U.S.C.I.S." never matches: `CARNET_ABBREVIATION` wants no letter or dot before.
fn carnet_abbreviation_is_an_id(label: &str) -> bool {
    CARNET_ABBREVIATION
        .find_iter(label)
        .any(|found| !glued_to_another_abbreviation(&label[found.end()..]))
        && !has_continuous_integration_context(label)
}

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
    let text = NUMBER_HINT.replace_all(label, " ");
    let text = TAX_BASIS.replace_all(&text, |caps: &regex::Captures| {
        let next = caps.get(1).map(|word| word.as_str().to_lowercase());
        match next {
            Some(word)
                if TAX_ID_WORDS.contains(&word.as_str())
                    || TAX_QUALIFIERS.contains(&word.as_str()) =>
            {
                caps[0].to_string()
            }
            Some(word) => format!(" {word}"),
            None => " ".to_string(),
        }
    });
    let text = LABEL_ASIDE.replace_all(&text, " ");
    let text = ORDINAL_NUMBER.replace_all(&text, " number ");
    let text = text.replace(['º', '°', '˚'], ".").replace('#', " number ");
    let text = YES_NO.replace_all(&text, " ");
    let text = DOTTED_NUMBER.replace_all(&text, "$1 number ");
    normalize(&fold_look_alikes(&text))
}

/// True when `label` is a field the program never fills: what `AnswerBank::lookup` asks before it
/// reads any entry, so that no bank pattern can answer it.
pub fn is_never_filled(label: &str) -> bool {
    // These two read the raw label: lowercased, "RUN" would be a verb and "C.I." a dotted noise.
    if capital_token_is_an_id(label) || carnet_abbreviation_is_an_id(label) {
        return true;
    }
    let text = read_label(label);
    let words: Vec<&str> = text.split_whitespace().collect();
    let whole_label = without_markers(&words).join(" ");
    BARE_WORD_LABELS.contains(&whole_label.as_str())
        || number_abbreviation_closes_a_peeled_label(label)
        || asks_for_a_document_after_the_aside(label, &text)
        || is_never_filled_after_reading(&without_named_document(&text))
}

/// A trailing remark in brackets, with the punctuation a form adds after it.
static TRAILING_ASIDE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\s*:.]*[(\[][^()\[\]]*[)\]][\s*:.]*$").expect("TRAILING_ASIDE is valid")
});

/// "Passport No (if any)", "Account no (for salary payment)": a final "No" followed only by
/// remarks in brackets is the abbreviation of "number" as much as one at the very end. Read on
/// the raw label, once for every remark peeled off the end.
fn number_abbreviation_closes_a_peeled_label(label: &str) -> bool {
    let mut rest = label;
    while let Some(aside) = TRAILING_ASIDE.find(rest) {
        rest = &rest[..aside.start()];
        let text = read_label(rest);
        let words: Vec<&str> = text.split_whitespace().collect();
        if ends_with_no_after_a_document(&words) {
            return true;
        }
    }
    false
}

/// "Full name (as on passport) and ID": a name label whose reference to a document sits in
/// brackets but which asks for a document on its own after them. The exemption would remove both.
fn asks_for_a_document_after_the_aside(label: &str, text: &str) -> bool {
    if !asks_for_a_name(text) || !NAMED_DOCUMENT.is_match(text) {
        return false;
    }
    label.match_indices([')', ']']).any(|(at, _)| {
        read_label(&label[at + 1..])
            .split_whitespace()
            .any(is_anchor)
    })
}

fn asks_for_a_name(text: &str) -> bool {
    ["name", "nombre", "surname", "apellido"]
        .iter()
        .any(|word| contains_phrase(text, word))
}

/// The words of a label without the required/optional markers a form adds after it.
fn without_markers<'a>(words: &[&'a str]) -> Vec<&'a str> {
    words
        .iter()
        .copied()
        .filter(|word| !REQUIRED_MARKERS.contains(word))
        .collect()
}

/// Every rule that works on a label already read (`read_label`) and, for a name field, already
/// freed of its document reference.
fn is_never_filled_after_reading(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    NEVER_FILLED_TERMS
        .iter()
        .any(|phrase| contains_phrase(text, &normalize(phrase)))
        || number_beside_document(&words)
        || number_beside_banking_noun(&words)
        || tax_then_number(&words)
        || account_details(&words)
        || ends_with_no_after_a_document(&words)
        || spanish_no_de_a_document(&words)
        || only_document_words(&words)
}

/// The word "tax", at most `MAX_TAX_QUALIFIERS` qualifiers, then a number-word: "Tax number", "Tax payer number",
/// "Tax Reg. No.", "Tax reference No" (a final "No" counts as in `ends_with_no_after_a_document`).
/// "Tax software, No. of years" has a word in between that is no qualifier and is not matched.
fn tax_then_number(words: &[&str]) -> bool {
    let words = without_markers(words);
    words.iter().enumerate().any(|(at, word)| {
        if *word != "tax" {
            return false;
        }
        (0..=MAX_TAX_QUALIFIERS).any(|qualifiers| {
            let number_at = at + 1 + qualifiers;
            let Some(candidate) = words.get(number_at) else {
                return false;
            };
            let qualified = words[at + 1..number_at]
                .iter()
                .all(|between| TAX_QUALIFIERS.contains(between));
            let is_final = number_at + 1 == words.len();
            qualified
                && (NUMBER_WORDS.contains(candidate)
                    || (is_final && ["no", "n", "nr"].contains(candidate)))
        })
    })
}

/// "Account details" is the bank-account block when it is the whole label (after markers and the
/// wrappers of `ACCOUNT_DETAILS_WRAPPERS`) or when a word of `BANK_CONTEXT_WORDS` is in the label.
/// "LinkedIn account details" is a social profile and stays answerable, so this cannot be a
/// phrase of `NEVER_FILLED_TERMS`.
fn account_details(words: &[&str]) -> bool {
    let words = without_markers(words);
    if !words.windows(2).any(|pair| pair == ["account", "details"]) {
        return false;
    }
    let unwrapped: Vec<&str> = words
        .iter()
        .copied()
        .skip_while(|word| ACCOUNT_DETAILS_WRAPPERS.contains(word))
        .collect();
    unwrapped == ["account", "details"]
        || words.iter().any(|word| BANK_CONTEXT_WORDS.contains(word))
}

/// Spanish writes the abbreviation of "número" as "No" or "N°", and without its dot it still
/// reads as a number when a document follows: "No de pasaporte".
fn spanish_no_de_a_document(words: &[&str]) -> bool {
    words.windows(3).any(|triple| {
        triple[0] == "no"
            && triple[1] == "de"
            && (is_document_word(triple[2]) || NUMBER_THEN_DE_NOUNS.contains(&triple[2]))
    })
}

/// A label made only of document words and glue words ("Your ID", "Passport or ID") has no word of its
/// own that would say it is not the document's number, as "LinkedIn ID" and "Job ID" do. It needs
/// at least one anchor: "Tax" or "Document" alone could be anything.
fn only_document_words(words: &[&str]) -> bool {
    // "Pasaporte (opcional)" is the document: the marker is not a word of the label.
    let words = without_markers(words);
    words
        .iter()
        .all(|word| is_anchor(word) || is_document_glue(word))
        && words.iter().any(|word| is_anchor(word))
}

/// Words a form adds after a label to say whether the field is required.
const REQUIRED_MARKERS: &[&str] = &[
    "required",
    "optional",
    "obligatorio",
    "opcional",
    "requerido",
];

/// A "no" or "n" that closes the label right after a document or banking word is the abbreviation
/// ("Passport No", "Account no"); anywhere else it is a plain word.
fn ends_with_no_after_a_document(words: &[&str]) -> bool {
    // A "(required)" or "*" after the label moves the "no" off the end; only for this check the
    // marker words are dropped ("*" and brackets are already gone after `normalize`).
    let kept = words
        .iter()
        .rposition(|word| !REQUIRED_MARKERS.contains(word))
        .map_or(0, |last| last + 1);
    let words = &words[..kept];
    let [.., before, last] = words else {
        return false;
    };
    let ends_a_document_term = || {
        DOCUMENTS
            .iter()
            .filter(|document| matches!(document.kind, Kind::Strong | Kind::Weak))
            .map(|document| document.term)
            .any(|term| term.rsplit(' ').next() == Some(*before))
    };
    ["no", "n", "nr"].contains(last)
        && (NOUNS_THEN_NUMBER.contains(before) || ends_a_document_term())
}

/// A name field names the document its name must match ("Full name (as on your national ID)"):
/// that is not the document's number, so the reference is taken out of the label before the rules
/// read it. Only when what is left has neither a number-word nor an anchor: "Name and number as on
/// your national ID" and "Name and ID as on your national ID" ask for more and are kept whole.
fn without_named_document(text: &str) -> String {
    // Decided on the label as read, before anything is removed: a label that already asks for a
    // number ("... , and your passport No") is never made answerable by taking a document out.
    let words: Vec<&str> = text.split_whitespace().collect();
    let asks_for_a_number = words.iter().any(|word| NUMBER_WORDS.contains(word))
        || ends_with_no_after_a_document(&words);
    if !asks_for_a_name(text) || asks_for_a_number {
        return text.to_string();
    }
    let rest = NAMED_DOCUMENT.replace_all(text, " ").into_owned();
    let says_more = |word: &str| NUMBER_WORDS.contains(&word) || is_anchor(word);
    if rest.split_whitespace().any(says_more) {
        text.to_string()
    } else {
        rest
    }
}

/// The first and last word index of every place where `phrase` (words separated by spaces) occurs
/// in `words`.
fn phrase_spans(words: &[&str], phrase: &str) -> Vec<(usize, usize)> {
    let parts: Vec<&str> = phrase.split(' ').collect();
    words
        .windows(parts.len())
        .enumerate()
        .filter(|(_, window)| *window == parts.as_slice())
        .map(|(first, _)| (first, first + parts.len() - 1))
        .collect()
}

/// Whether the word at `index` is at most `MAX_WORDS_APART` words from a phrase that occupies the
/// words `first..=last`.
fn within_reach(index: usize, (first, last): (usize, usize)) -> bool {
    let apart = if index < first {
        first - index
    } else {
        index.saturating_sub(last)
    };
    apart <= MAX_WORDS_APART
}

/// A strong document with a number-word anywhere, or a weak one with a number-word within reach,
/// in either order.
fn number_beside_document(words: &[&str]) -> bool {
    let numbers: Vec<usize> = (0..words.len())
        .filter(|&index| NUMBER_WORDS.contains(&words[index]))
        .collect();
    if numbers.is_empty() {
        return false;
    }
    terms_of(Kind::Strong).any(|term| !phrase_spans(words, term).is_empty())
        || terms_of(Kind::Weak).any(|term| {
            phrase_spans(words, term)
                .into_iter()
                .any(|span| numbers.iter().any(|&index| within_reach(index, span)))
        })
}

/// "account number", "acct #", "card no" (noun, then number-word) and the Spanish "número de
/// cuenta", "N° de tarjeta" (number-word, "de", noun).
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
        "Acct #",
        "N° de cuenta",
        "Nro. de cuenta",
        "Card number",
        "Card no.",
        "Número de tarjeta",
        // Terms that are never filled alone.
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
        // More terms that are never filled alone.
        "R.U.N.",
        "SWIFT BIC",
        "VAT ID",
        "IFSC",
        "CLABE",
        "Tax code",
        // The word "tax", at most one qualifier, then a number-word.
        "Tax identification (TIN/SSN)",
        "Tax Nr",
        "Tax reference number",
        "Tax registration number",
        "Tax payer number",
        "Tax identifier number",
        "Tax residence number",
        "Tax Reg. No.",
        "Tax reg #",
        "Tax Ref. No.",
        "Tax File Number (TFN)",
        // The plural of a number-word is a number-word.
        "Document numbers",
        "ID document numbers",
        "Visa grant numbers",
        "Medicare numbers",
        // Bank wording of "account details": whole label, wrapped, or with a payment word.
        "Please enter your account details",
        "Enter account details",
        "Account details for payment",
        "Payment account details",
        "Your account details (bank)",
        "Account details (for salary deposit)",
        // The basis wording of a salary never hides a real tax ID that follows "tax".
        "Full name, including tax ID",
        "Contact details including tax number",
        "Billing details including tax number",
        // A final "No" followed only by a parenthetical is still a number.
        "Passport No (if any)",
        "Passport No (for visa purposes)",
        "Account no (for salary payment)",
        "Name & passport no (as on passport)",
        // A document asked for on its own after the closing parenthesis of a name label.
        "Full name (as on passport) and ID",
        // Two tax qualifiers in a row.
        "Tax payer registration number",
        "Tax payer reference number",
        "Tax registration ref no.",
        // A format hint is not a number-word, but the document or number beside it still counts.
        "Passport (numbers only)",
        "ID number (numbers only)",
        "Tax number (numbers only)",
    ];

    /// Terms that are never filled alone by phrase (any case), in Chilean, foreign and banking
    /// wording.
    const MORE_NEVER_FILLED_TERMS: &[&str] = &[
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
        "Bank account details",
        "RFC (Registro Federal de Contribuyentes)",
        "Registro Federal de Contribuyentes (RFC)",
    ];

    /// "RFC" is the tax ID only in a short label (four words at most): in a longer one it is the
    /// engineering document. Tried as written.
    const SHORT_RFC_LABELS: &[&str] = &["RFC", "RFC (con homoclave)", "Ingrese su RFC"];

    /// Never filled only as the whole label (after a required/optional marker): "account details"
    /// inside a longer label is a social profile.
    const WHOLE_LABEL_TERMS: &[&str] = &["Account details", "Account details (required)"];

    /// Never filled through a token written in capitals or with dots in the RAW label ("RUN",
    /// "C.I."): lowercase "run" is a verb, so these are tried as written, not lowercased.
    const RAW_TOKEN_LABELS: &[&str] = &[
        "RUN (sin puntos ni guión)",
        "RUN (ej: 12345678-9)",
        "Ingrese su RUN",
        "RUN del postulante",
        "C.I.",
        "c.i.",
        "C.I. N°",
        "Su C.I.",
        "RUN",
        "RUN DEL POSTULANTE",
        "INGRESE SU RUN",
        "NUMERO DE RUN",
        "RUN (SIN PUNTOS NI GUION)",
        "ENTER YOUR TIN",
        // A standalone "C.I." is the ID card unless it is glued to another abbreviation or the
        // label talks about continuous integration.
        "C.I. (sin puntos ni guión)",
        "C.I. (sin puntos)",
        "C.I. (ej: 12345678-9)",
        "C.I. o pasaporte",
        "C.I./Pasaporte",
        "Pasaporte o C.I.",
        "C.I. del postulante",
        "Indique C.I.",
        "Indique C.I. y fecha de nacimiento",
        "Nro. C.I.",
        "No. C.I.",
        "Nº C.I.",
        "Documento: C.I.",
        "Identificación (C.I.)",
        "C.I: 12345678",
        // A label of at most three words written in capitals: the token counts wherever it is.
        "CHILEAN RUN",
        "INDIQUE RUN",
        "NRO RUN",
        "NRO. RUN",
        "VALID TIN",
        "TAXPAYER TIN",
        "NIT",
        "RFC",
        "TIN",
        "CBU",
        "CUIT",
        "CUIL",
    ];

    /// Never filled on purpose although they could be read as ordinary: that is the safe side.
    /// "Job ID number" and "Employee ID number" match "id number", "bank accounts" matches "bank
    /// account".
    const NEVER_FILLED_ON_PURPOSE: &[&str] = &[
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

    /// Examples of `BARE_WORD_LABELS` as a form writes them: a section caption such as "Passport"
    /// may be missing from the label the scanner reports, which is then just this word.
    const BARE_WORD_LABEL_EXAMPLES: &[&str] = &[
        "Number", "No", "Numero", "Número", "Nro", "Nro.", "Num", "ID", "N", "Number *", "No.",
    ];

    /// Labels that only look like the ones above: none of them may be never filled.
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
        "Hourly rate (excl. tax), numbers only",
        "Expected salary including tax (number)",
        // Format hints are not number-words.
        "Years with ID verification (numbers only)",
        "Years of experience with identity platforms (numbers only)",
        "Full name as in your passport (no numbers or symbols)",
        "Hourly rate (excl. VAT), numbers only",
        "Daily rate in EUR (excl. VAT) - numbers only",
        "Expected salary (with tax), numbers only",
        "Expected salary (excluding VAT/tax), numbers only",
        // Capitals from a form that styles labels in uppercase, with the verb "run" or an RFC the
        // engineer wrote.
        "HOW MANY YEARS HAVE YOU RUN PRODUCTION SYSTEMS?",
        "WILLING TO RUN ON-CALL?",
        "HAVE YOU RUN A TEAM?",
        "HOW DO YOU RUN TESTS?",
        "DESCRIBE AN RFC YOU WROTE",
        "Have you ever authored an RFC?",
        "Describe an RFC you wrote and how it was received",
        // "Account details" inside a longer label is a social profile, not a bank account.
        "LinkedIn account details",
        "Please share your GitHub account details",
        "Social media account details",
        // "C.I." as the continuous-integration abbreviation, or inside another abbreviation.
        "C.I./C.D. experience",
        "C.I. pipelines you have built",
        "U.S.C.I.S. status",
        "Experience with C.I. tools",
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

    /// Name fields that only say which document the name must match. Each one is never filled
    /// WITHOUT the name exemption (it has a term that is never filled alone), so it proves the
    /// exemption.
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

    /// Name fields answerable with or without the exemption: they have no term that is never
    /// filled alone and no number-word, so they say nothing about the exemption.
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
    /// the abbreviation of "number", so none of these may be never filled.
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
        "Tax reference No",
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
    fn a_term_that_is_never_filled_alone_is_in_any_case_and_with_a_marker() {
        for label in MORE_NEVER_FILLED_TERMS {
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
    fn a_token_in_capitals_or_with_dots_is_never_filled_as_written() {
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
    fn the_name_exemption_never_eats_the_document_that_precedes_a_final_number() {
        let bases = [
            "Name (as on passport) / Passport",
            "Name exactly as on your passport, and your passport",
            "Name as on your ID, and ID",
        ];
        for base in bases {
            for ending in [" No", " N", " Nr", " Nº", " #"] {
                let label = format!("{base}{ending}");
                for shown in [label.clone(), format!("Please enter: {label}")] {
                    let bank = loose_and_exact_bank(&shown);
                    assert!(
                        bank.lookup(&shown).is_none(),
                        "a bank entry answered {shown:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_short_rfc_is_a_tax_id_and_a_long_label_with_an_rfc_is_not() {
        for label in SHORT_RFC_LABELS {
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_none(), "answered {label:?}");
        }
        for label in [
            "DESCRIBE AN RFC YOU WROTE",
            "Describe an RFC you wrote and how it was received",
        ] {
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_some(), "never filled {label:?}");
        }
    }

    #[test]
    fn account_details_are_never_filled_only_as_the_whole_label() {
        for label in WHOLE_LABEL_TERMS {
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_none(), "answered {label:?}");
        }
    }

    #[test]
    fn a_label_never_filled_on_purpose_is_never_filled() {
        for label in NEVER_FILLED_ON_PURPOSE {
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_none(), "answered {label:?}");
        }
    }

    #[test]
    fn a_label_made_only_of_document_words_is_never_filled() {
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
    fn a_name_label_that_proves_the_exemption_is_never_filled_without_it() {
        for label in NAME_EXEMPTION_PROOF {
            assert!(
                is_never_filled_after_reading(&read_label(label)),
                "{label:?} would be answerable without the exemption: it proves nothing"
            );
        }
    }

    #[test]
    fn the_name_exemption_answers_a_name_label_that_cites_a_document() {
        for label in NAME_EXEMPTION_PROOF {
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_some(), "{label:?} was never filled");
        }
    }

    #[test]
    fn a_name_label_answerable_either_way_needs_no_exemption() {
        for label in NAME_ANSWERABLE_EITHER_WAY {
            assert!(
                !is_never_filled_after_reading(&read_label(label)),
                "{label:?} belongs with the proofs: it is never filled without the exemption"
            );
            let bank = loose_and_exact_bank(label);
            assert!(bank.lookup(label).is_some(), "{label:?} was never filled");
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
                "{label:?} was never filled by a plain \"no\" or \"n\""
            );
        }
    }

    #[test]
    fn a_label_that_is_only_a_bare_number_word_is_never_filled() {
        for label in BARE_WORD_LABEL_EXAMPLES {
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
                "{label:?} was never filled by the list"
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
