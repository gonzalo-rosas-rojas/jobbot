//! The region filter: could Gonza take this job from Chile?
//!
//! A job that is not remote needs a place in Chile. A remote job is read in this order:
//!
//! 1. A title that names LATAM keeps it open.
//! 2. The location is split into places (`;`, `|`, `/`, line breaks) and each place is read on its
//!    own: open (Chile, any Latin American country or city, "Americas", "Worldwide", ...), closed
//!    (a US state or big city, Canada, Europe, Asia, ...) or neither ("Remote", "Springfield").
//!    One open place opens the job; otherwise one closed place closes it. A place the lists do
//!    not know never decides: a list of Canadian cities with one the lists miss is still Canada.
//! 3. A location that names no known place leaves the description to decide, and only a sentence
//!    that confines the job to one country ("must be located in the US", "authorized to work in
//!    the United States") closes it.
//!
//! Every doubt resolves toward keeping the job: a wrongly dropped job is never seen again, a
//! wrongly kept one costs a look.

use std::sync::LazyLock;

use regex::{Regex, RegexSet};

use crate::models::Job;

/// Words that open a job to Chile: all of Latin America or no limit at all, and the Latin American
/// countries and big cities (one of them in a list of places is enough, as in Remotive's
/// "USA, Canada, Argentina, Mexico, Peru").
const OPEN_TERMS: &[&str] = &[
    "latam",
    "latin america",
    "south america",
    "americas",
    "amer",
    "worldwide",
    "world wide",
    "global",
    "globally",
    "anywhere",
    "rest of the world",
    "rest of world",
    "any country",
    "chile",
    "argentina",
    "bolivia",
    "brazil",
    "brasil",
    "colombia",
    "costa rica",
    "dominican republic",
    "ecuador",
    "el salvador",
    "guatemala",
    "honduras",
    "mexico",
    "nicaragua",
    "panama",
    "paraguay",
    "peru",
    "uruguay",
    "venezuela",
    "santiago",
    "valparaiso",
    "concepcion",
    "buenos aires",
    "bogota",
    "medellin",
    "lima",
    "sao paulo",
    "rio de janeiro",
    "mexico city",
    "ciudad de mexico",
    "montevideo",
    "quito",
    "guadalajara",
    "curitiba",
];

/// The US as a country, as a region and by state and big city. Bare "america" is left out on
/// purpose: it would also swallow "Latin America".
const US_TERMS: &[&str] = &[
    "us",
    "usa",
    "united states",
    "united states of america",
    "north america",
    "northern america",
    "continental us",
    "east coast",
    "west coast",
    "midwest",
    "northeast",
    "southeast",
    "southwest",
    "pacific northwest",
    "new england",
    "tri-state",
    "bay area",
    "silicon valley",
    "nyc",
    "sf",
    "dc",
    "district of columbia",
    "alabama",
    "alaska",
    "arizona",
    "arkansas",
    "california",
    "colorado",
    "connecticut",
    "delaware",
    "florida",
    "georgia",
    "hawaii",
    "idaho",
    "illinois",
    "indiana",
    "iowa",
    "kansas",
    "kentucky",
    "louisiana",
    "maine",
    "maryland",
    "massachusetts",
    "michigan",
    "minnesota",
    "mississippi",
    "missouri",
    "montana",
    "nebraska",
    "nevada",
    "new hampshire",
    "new jersey",
    "new mexico",
    "new york",
    "north carolina",
    "north dakota",
    "ohio",
    "oklahoma",
    "oregon",
    "pennsylvania",
    "rhode island",
    "south carolina",
    "south dakota",
    "tennessee",
    "texas",
    "utah",
    "vermont",
    "virginia",
    "washington",
    "west virginia",
    "wisconsin",
    "wyoming",
    "san francisco",
    "los angeles",
    "san diego",
    "san jose",
    "san mateo",
    "san antonio",
    "seattle",
    "austin",
    "boston",
    "chicago",
    "denver",
    "atlanta",
    "miami",
    "dallas",
    "houston",
    "phoenix",
    "scottsdale",
    "portland",
    "philadelphia",
    "pittsburgh",
    "minneapolis",
    "salt lake city",
    "raleigh",
    "nashville",
    "charlotte",
    "detroit",
    "las vegas",
    "tampa",
    "orlando",
    "panama city beach",
    "boulder",
    "oakland",
    "brooklyn",
    "palo alto",
    "mountain view",
    "menlo park",
    "redwood city",
    "foster city",
    "sunnyvale",
    "cupertino",
    "santa clara",
    "santa monica",
    "burlingame",
    "irvine",
    "sacramento",
    "honolulu",
    "columbus",
    "cincinnati",
    "baltimore",
    "st louis",
    "kansas city",
    "indianapolis",
    "richmond",
    "savannah",
];

/// Canada: the country, its provinces and big cities.
const CANADA_TERMS: &[&str] = &[
    "canada",
    "canadian",
    "ontario",
    "quebec",
    "british columbia",
    "alberta",
    "nova scotia",
    "manitoba",
    "saskatchewan",
    "new brunswick",
    "newfoundland",
    "prince edward island",
    "toronto",
    "vancouver",
    "montreal",
    "ottawa",
    "calgary",
    "edmonton",
    "winnipeg",
    "halifax",
    "mississauga",
];

/// The rest of the world outside the Americas: continents, regions, countries and big cities
/// (Remotive and Ashby write "Europe", "APAC", "Remote - Japan", "Dublin").
const ABROAD_TERMS: &[&str] = &[
    "europe",
    "european union",
    "eu",
    "emea",
    "cet",
    "apj",
    "noram",
    "dach",
    "nordics",
    "benelux",
    "saarc",
    "gcc",
    "anz",
    "asean",
    "ksa",
    "luxembourg",
    "cyprus",
    "malta",
    "uk",
    "united kingdom",
    "great britain",
    "england",
    "scotland",
    "wales",
    "ireland",
    "apac",
    "asia",
    "asia pacific",
    "southeast asia",
    "australia",
    "new zealand",
    "nz",
    "auckland",
    "wellington",
    "christchurch",
    "oceania",
    "india",
    "middle east",
    "mena",
    "africa",
    "germany",
    "france",
    "spain",
    "italy",
    "portugal",
    "netherlands",
    "belgium",
    "switzerland",
    "austria",
    "sweden",
    "norway",
    "denmark",
    "finland",
    "iceland",
    "poland",
    "czechia",
    "czech republic",
    "romania",
    "bulgaria",
    "greece",
    "hungary",
    "estonia",
    "latvia",
    "lithuania",
    "ukraine",
    "serbia",
    "croatia",
    "slovenia",
    "slovakia",
    "turkey",
    "turkiye",
    "israel",
    "united arab emirates",
    "uae",
    "saudi arabia",
    "qatar",
    "egypt",
    "nigeria",
    "kenya",
    "south africa",
    "ghana",
    "morocco",
    "japan",
    "china",
    "korea",
    "south korea",
    "singapore",
    "hong kong",
    "taiwan",
    "thailand",
    "vietnam",
    "philippines",
    "indonesia",
    "malaysia",
    "pakistan",
    "bangladesh",
    "sri lanka",
    "london",
    "dublin",
    "paris",
    "berlin",
    "munich",
    "amsterdam",
    "madrid",
    "barcelona",
    "lisbon",
    "stockholm",
    "copenhagen",
    "oslo",
    "helsinki",
    "warsaw",
    "prague",
    "vienna",
    "zurich",
    "geneva",
    "brussels",
    "tel aviv",
    "dubai",
    "abu dhabi",
    "riyadh",
    "tokyo",
    "seoul",
    "sydney",
    "melbourne",
    "bangalore",
    "bengaluru",
    "mumbai",
    "delhi",
    "hyderabad",
    "pune",
    "shanghai",
    "beijing",
    "shenzhen",
    "taipei",
    "manila",
    "jakarta",
    "bangkok",
    "hanoi",
    "kuala lumpur",
    "istanbul",
    "cairo",
    "lagos",
    "nairobi",
    "cape town",
    "johannesburg",
    "krakow",
    "bucharest",
    "sofia",
    "belgrade",
];

/// "City, ST": the US state and Canadian province codes, read in the original capitals (`CA`,
/// not the word "ca"). `CO` and `AR` are left out: they are Colombia and Argentina as often as
/// Colorado and Arkansas, and those two states are caught by name and by their cities.
const STATE_CODES: &str = "AL|AK|AZ|CA|CT|DE|FL|GA|HI|ID|IL|IN|IA|KS|KY|LA|ME|MD|MA|MI|MN|MS|MO|MT|NE|NV|NH|NJ|NM|NY|NC|ND|OH|OK|OR|PA|RI|SC|SD|TN|TX|UT|VT|VA|WA|WV|WI|WY|DC|ON|BC|AB|QC|NS|NB|MB|SK|NL";

/// The pattern for "one of these words or phrases, as whole words".
fn any_word(terms: &[&str]) -> String {
    let mut terms = terms.to_vec();
    // The longest first, so "new york city" is read before "new york".
    terms.sort_by_key(|term| std::cmp::Reverse(term.len()));
    let escaped: Vec<String> = terms.iter().map(|term| regex::escape(term)).collect();
    format!(r"\b(?:{})\b", escaped.join("|"))
}

/// Every place name that closes a job: the US, Canada and the rest of the world.
static RESTRICTED: LazyLock<Regex> = LazyLock::new(|| {
    let all: Vec<&str> = [US_TERMS, CANADA_TERMS, ABROAD_TERMS].concat();
    Regex::new(&any_word(&all)).expect("RESTRICTED is a valid regex")
});

static OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&any_word(OPEN_TERMS)).expect("OPEN is a valid regex"));

/// "Anywhere in the US" is not anywhere: read it as the US.
static ANYWHERE_IN_RESTRICTED: LazyLock<Regex> = LazyLock::new(|| {
    let all: Vec<&str> = [US_TERMS, CANADA_TERMS, ABROAD_TERMS].concat();
    let places = any_word(&all);
    Regex::new(&format!(
        r"\b(?:anywhere|everywhere)\s+(?:in|within|across)\s+(?:the\s+)?{places}"
    ))
    .expect("ANYWHERE_IN_RESTRICTED is a valid regex")
});

static STATE_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r",\s*(?:{STATE_CODES})\b")).expect("STATE_CODE is a valid regex")
});

/// A title that says the job is for Latin America.
static LATAM_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:latam|latin america)\b").expect("LATAM_TITLE is a valid regex")
});

/// The countries and regions a description may confine a job to.
const CONFINING_PLACES: &str = r"(?:us|usa|united states(?: of america)?|canada|europe|the eu|eu|uk|united kingdom|emea|apac|india|australia)";
const US_PLACES: &str = r"(?:us|usa|united states(?: of america)?|continental us|contiguous us)";

/// Sentences that confine a job to one country, read after `fold`. Each one needs the words that
/// make it about the candidate ("must", "only", "authorized to work"): a company that is "US-based"
/// or a benefit "for US residents" says nothing about who may apply.
static CONFINING: LazyLock<RegexSet> = LazyLock::new(|| {
    let candidate = r"(?:must|need to|needs to|have to|has to|required to|should|will need to)";
    let negative = r"(?:cannot|can not|can't|unable to|do not|don't|will not|won't|not able to)";
    RegexSet::new([
        // "must be located in US East, Central, or West timezone", "must reside in the United States"
        format!(
            r"\b{candidate}\s+(?:also\s+)?(?:be\s+)?(?:currently\s+|physically\s+|legally\s+)?(?:located|reside|residing|resident|living|live|based|work(?:ing)? from)\s+(?:in|within|from)\s+(?:the\s+)?{CONFINING_PLACES}\b"
        ),
        // "can be based remotely anywhere in the United States", "remote within the US"
        format!(r"\b(?:anywhere|everywhere|remotely|remote)\s+(?:in|within|across)\s+(?:the\s+)?{US_PLACES}\b"),
        // "US-based candidates only", "this is a US-based role", "must be US-based". Candidates
        // alone are not enough: "For US-based applicants: this position is part of the union"
        // is a note for some applicants of a global job.
        format!(
            r"\b{US_PLACES}\s*-?\s*based\s+(?:(?:candidates?|applicants?|talent|residents?)\s+only|only|role|position|opportunity|remote)\b"
        ),
        format!(r"\b(?:must|need to|have to|should)\s+be\s+(?:a\s+)?{US_PLACES}[- ]based\b"),
        // "authorized to work in the United States", "US work authorization"
        format!(
            r"\b(?:authori[sz]ed|eligible|permitted|entitled|right)\s+to\s+work\s+(?:in|within)\s+(?:the\s+)?{US_PLACES}\b"
        ),
        format!(r"\b{US_PLACES}\s+work\s+authori[sz]ation\b"),
        // "U.S. citizenship is required"
        format!(r"\b{US_PLACES}\s+citizen(?:s|ship)?\b"),
        // "we are only hiring in the US"
        format!(
            r"\b(?:only|solely|exclusively)\s+(?:hiring|hire|accepting|accept|considering|consider)\b[^.]{{0,60}}?\b(?:in|from|within)\s+(?:the\s+)?{CONFINING_PLACES}\b"
        ),
        // "open to candidates located in the United States"
        format!(
            r"\b(?:open|available|limited|restricted)\s+(?:only\s+)?to\s+(?:candidates|applicants|residents|individuals|people)\s+(?:who\s+(?:are\s+)?(?:located|based|living|residing)\s+)?(?:located\s+|based\s+|residing\s+|living\s+)?(?:in|within)\s+(?:the\s+)?{CONFINING_PLACES}\b"
        ),
        // "we are unable to hire candidates outside the US"
        format!(
            r"\b{negative}\s+(?:currently\s+)?(?:hire|consider|accept|employ|support)\b[^.]{{0,60}}?\boutside\s+(?:of\s+)?(?:the\s+)?{US_PLACES}\b"
        ),
        // "US residents only"
        format!(r"\b{US_PLACES}\s+(?:residents?|citizens?)\s+only\b"),
    ])
    .expect("CONFINING is a valid set of regexes")
});

/// Sentences of a posting based in one of the extra places that say the candidate must already
/// have the right to work there, read after `fold`. They close the job unless the posting also
/// offers sponsorship or relocation (`SPONSOR_OFFER`): the user has no work rights there.
/// A posting open to Chile never goes through these: "no visa sponsorship" in a remote contractor
/// posting says nothing about someone who works from Chile.
static NO_WORK_RIGHTS: LazyLock<RegexSet> = LazyLock::new(|| {
    let rights = r"(?:work(?:ing)?\s+rights|right\s+to\s+work|authori[sz]ed\s+to\s+work|eligible\s+to\s+work|permitted\s+to\s+work|work\s+eligibility|work\s+authori[sz]ation)";
    RegexSet::new([
        // "valid Australian work rights"
        format!(r"\b(?:valid|full|existing|unrestricted|current|legal|unconditional)\s+(?:australian|aus|nz|new\s+zealand)\s+{rights}\b"),
        // "eligible to work in Australia", "the right to work in Australia"
        format!(r"\b{rights}\s+(?:in|for)\s+(?:the\s+)?(?:australia|new\s+zealand)\b"),
        // "must have the right to work ... in Australia"
        format!(r"\bmust\s+(?:already\s+)?(?:have|hold|possess)\b[^.]{{0,40}}\b{rights}\b"),
        // "Australian citizens only", "must be an Australian citizen or permanent resident"
        r"\baustralian\s+citizen(?:s|ship)?\b[^.]{0,40}\b(?:only|required|must)\b".to_string(),
        r"\bmust\s+be\s+(?:an?\s+)?australian\s+(?:citizen|permanent\s+resident)".to_string(),
        // "does not currently offer visa sponsorship", "no sponsorship", "unable to sponsor"
        r"\b(?:does|do|will|can|are|is)\s*(?:not|n't)\s+(?:currently\s+)?(?:able\s+to\s+)?(?:offer|provide|support)\s+(?:any\s+)?(?:visa\s+)?(?:sponsorship|visas?|work\s+permits?)\b".to_string(),
        r"\b(?:does|do|will|can|are|is)\s*(?:not|n't)\s+(?:currently\s+)?(?:able\s+to\s+)?sponsor\b".to_string(),
        r"\b(?:cannot|can\s*not|can't)\s+(?:currently\s+)?sponsor\b".to_string(),
        r"\bno\s+(?:visa\s+|work\s+permit\s+)?sponsorship\b".to_string(),
        r"\b(?:unable|not\s+able)\s+to\s+(?:offer\s+|provide\s+)?sponsor".to_string(),
        r"\bsponsorship\s+(?:is\s+)?not\s+(?:available|offered|provided)\b".to_string(),
    ])
    .expect("NO_WORK_RIGHTS is a valid set of regexes")
});

/// A sentence that offers to help a candidate from abroad ("We do sponsor visas!", "We offer
/// relocation assistance."), which rescues a posting that also holds a `NO_WORK_RIGHTS` sentence.
static SPONSOR_OFFER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:we|company|employer)\s+(?:do|can|will|may|are\s+able\s+to|are\s+happy\s+to)\s+(?:also\s+)?sponsor\b|\bsponsorship\s+(?:is\s+|may\s+be\s+|can\s+be\s+)?(?:available|provided|offered|possible)\b|\brelocation\s+(?:assistance|support|package|allowance|bonus|budget|help)\b|\b(?:visa|immigration)\s+support\b",
    )
    .expect("SPONSOR_OFFER is a valid regex")
});

/// A negation, so that "we do not offer relocation assistance" is not read as an offer.
static NEGATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:no|not|without|cannot|unable|never)\b|n't\b")
        .expect("NEGATION is a valid regex")
});

/// Whether a posting based in an extra place demands work rights it does not help to get.
fn demands_work_rights(description: &str) -> bool {
    let folded = fold(description);
    let sentences: Vec<&str> = folded.split(['.', '!', '?', '\n', ';']).collect();
    sentences.iter().any(|s| NO_WORK_RIGHTS.is_match(s))
        && !sentences
            .iter()
            .any(|s| SPONSOR_OFFER.is_match(s) && !NEGATION.is_match(s))
}

/// The extra places of `criteria.toml` as one whole-word pattern; `None` when there are none.
fn extra_places(terms: &[String]) -> Option<Regex> {
    let folded: Vec<String> = terms
        .iter()
        .map(|term| fold(term.trim()))
        .filter(|term| !term.is_empty())
        .collect();
    if folded.is_empty() {
        return None;
    }
    let refs: Vec<&str> = folded.iter().map(String::as_str).collect();
    Some(Regex::new(&any_word(&refs)).expect("escaped terms make a valid regex"))
}

/// What one place of a location text means for someone living in Chile.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    /// Chile, a Latin American place or a place with no limit.
    Open,
    /// One of the extra places the user would move to (`criteria.extra_open_places`).
    Extra,
    /// The US, Canada or somewhere else outside the Americas.
    Closed,
    /// Neither: "Remote", "Hybrid", or a place the lists do not know ("Springfield").
    Neutral,
}

/// Lowercase text with accents removed and dotted abbreviations written plain (`U.S.` is `us`,
/// `D.C.` is `dc`), so that the word lists need neither accents nor dots.
fn fold(text: &str) -> String {
    let plain: String = text
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            '\u{2018}' | '\u{2019}' => '\'',
            other => other,
        })
        .collect();
    plain
        .replace("u.s.a", "usa")
        .replace("u.s.", "us")
        .replace("u.k.", "uk")
        .replace("d.c.", "dc")
}

/// Whether the folded text has an open word that is not part of a closed place: "New Mexico" is
/// not Mexico, and "anywhere in the US" is not anywhere.
fn has_open_term(folded: &str) -> bool {
    let masked = ANYWHERE_IN_RESTRICTED.replace_all(folded, " ");
    let masked = RESTRICTED.replace_all(&masked, " ");
    OPEN.is_match(&masked)
}

fn classify(place: &str, extra: Option<&Regex>) -> Place {
    let folded = fold(place);
    if has_open_term(&folded) {
        return Place::Open;
    }
    if extra.is_some_and(|extra| extra.is_match(&folded)) {
        return Place::Extra;
    }
    if RESTRICTED.is_match(&folded)
        || ANYWHERE_IN_RESTRICTED.is_match(&folded)
        || STATE_CODE.is_match(place)
    {
        return Place::Closed;
    }
    Place::Neutral
}

/// What the description says about where the job may be done.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Confinement {
    /// No sentence confines the job.
    None,
    /// A sentence confines it to a country the user would not move to (or to the US).
    Closed,
    /// A sentence confines it to one of the extra places, so it is a job there.
    ExtraPlace,
}

/// Whether a sentence of the description confines the job to one country, unless the same
/// sentence also names Latin America or no limit ("must be located in the US, Canada or LATAM").
/// A country in `extra` does not close the job: it makes it a job in that place.
fn description_confinement(description: &str, extra: Option<&Regex>) -> Confinement {
    let mut found = Confinement::None;
    for sentence in fold(description).split(['.', '!', '?', '\n']) {
        if !CONFINING.is_match(sentence) || has_open_term(sentence) {
            continue;
        }
        if extra.is_some_and(|extra| extra.is_match(sentence)) {
            found = Confinement::ExtraPlace;
        } else {
            return Confinement::Closed;
        }
    }
    found
}

/// Whether Gonza could take this job from Chile. See the module comment for the rules. Empty
/// fields never exclude a remote job by accident.
pub fn region_ok(job: &Job) -> bool {
    region_ok_with(job, &[])
}

/// Whether the posting's location names one of the extra places of `criteria.toml`, which is
/// what earns it the extra apply threshold (`criteria.extra_open_apply_threshold`).
pub fn in_extra_place(job: &Job, extra_places_open: &[String]) -> bool {
    extra_places(extra_places_open).is_some_and(|extra| extra.is_match(&fold(&job.location)))
}

/// Whether the title holds one of `criteria.extra_open_title_words` as a whole word; with no words
/// configured every title does.
pub fn title_earns_extra_threshold(job: &Job, words: &[String]) -> bool {
    match extra_places(words) {
        None => true,
        Some(words) => words.is_match(&fold(&job.title)),
    }
}

/// `region_ok` with the extra places of `criteria.toml` (`extra_open_places`) open too: a job
/// there is kept, on site or remote, unless it demands work rights it does not help to
/// get (`NO_WORK_RIGHTS` without a `SPONSOR_OFFER`). Without extra places it is `region_ok`.
pub fn region_ok_with(job: &Job, extra_places_open: &[String]) -> bool {
    let extra = extra_places(extra_places_open);
    if !job.remote {
        let location = job.location.to_lowercase();
        if location.contains("chile") || location.contains("santiago") {
            return true;
        }
        let in_extra_place = extra
            .as_ref()
            .is_some_and(|extra| extra.is_match(&fold(&job.location)));
        return in_extra_place && !demands_work_rights(&job.description);
    }
    if LATAM_TITLE.is_match(&fold(&job.title)) {
        return true;
    }
    let mut closed = false;
    let mut in_extra_place = false;
    for place in job.location.split([';', '|', '/', '\n']) {
        match classify(place, extra.as_ref()) {
            Place::Open => return true,
            Place::Extra => in_extra_place = true,
            Place::Closed => closed = true,
            Place::Neutral => {}
        }
    }
    if !in_extra_place && closed {
        return false;
    }
    if !in_extra_place {
        in_extra_place = match description_confinement(&job.description, extra.as_ref()) {
            Confinement::Closed => return false,
            Confinement::ExtraPlace => true,
            Confinement::None => false,
        };
    }
    !(in_extra_place && demands_work_rights(&job.description))
}

#[cfg(test)]
mod tests {
    use super::{region_ok, region_ok_with};
    use crate::testing::make_job_with;

    /// Whether a job with this title, location, remote flag and description passes the filter.
    fn passes(title: &str, location: &str, remote: bool, description: &str) -> bool {
        region_ok(&make_job_with(|job| {
            job.title = title.into();
            job.location = location.into();
            job.remote = remote;
            job.description = description.into();
        }))
    }

    /// Remote job with this location and no description.
    fn passes_at(location: &str) -> bool {
        passes("Senior Software Engineer", location, true, "")
    }

    /// Remote job at the plain location "Remote": only the description can decide.
    fn passes_saying(description: &str) -> bool {
        passes("Senior Software Engineer", "Remote", true, description)
    }

    fn assert_closed(locations: &[&str]) {
        for location in locations {
            assert!(
                !passes_at(location),
                "{location:?} cannot be done from Chile"
            );
        }
    }

    fn assert_open(locations: &[&str]) {
        for location in locations {
            assert!(passes_at(location), "{location:?} is open to Chile");
        }
    }

    #[test]
    fn a_us_state_or_city_is_a_us_job_even_when_it_says_remote() {
        // Every location below is a real one from the first run's top 60 (2026-10-03).
        assert_closed(&[
            "Remote - California",
            "Remote - Texas",
            "Remote - Illinois",
            "Maryland; Remote - Virginia; Remote - Washington D.C.",
            "Nevada; Remote - California; Remote - Colorado; Remote - Oregon; Remote - Washington",
            "Remote - California; San Francisco, California",
            "Foster City, CA",
            "NYC (SoHo)",
            "New York, New York",
            "New York, NY (HQ)",
            "San Francisco",
            "Austin, TX",
            "Dallas, TX",
            "Los Angeles, CA",
            "Washington, DC",
            "Remote-Friendly (Travel Required) | San Francisco, CA",
            "Remote-Friendly (Travel-Required) | San Francisco, CA | Seattle, WA | New York City, NY",
            "Atlanta, GA - Hybrid; Austin, TX - Remote; Chicago, IL - Remote; Denver, CO - Hybrid; \
             Los Angeles, CA - Remote; Miami, FL - Remote; New York, NY - Hybrid; \
             San Francisco, CA - Hybrid; Seattle, WA - Remote",
            "NYC, Remote; San Francisco, California",
            // A Florida beach town is no part of Panama (the Python Job Board writes it so).
            "Panama City Beach, Florida, United States",
        ]);
        // A region of the US, or the whole of North America (Mexico is rarely meant).
        assert_closed(&[
            "Remote - East Coast",
            "Remote - Pacific Northwest",
            "Bay Area",
            "Remote - North America",
            "Remote (US)",
            "Remote - USA",
            "US - Remote",
            "Remote - New Mexico",
        ]);
    }

    #[test]
    fn a_list_of_places_all_in_the_us_and_canada_is_closed() {
        assert_closed(&[
            // Cohere's Toronto posting as the source now writes it.
            "Toronto; Canada; United States; New York; Montreal",
            "Toronto, Ontario - Remote; New York, NY - Hybrid",
            "Ontario - Remote; Canada",
            "Toronto",
            "Vancouver, BC, Canada",
            "United States; Canada",
            // Real replay (2026-10-04): a place the lists do not know ("USCA", "Victoria") among
            // known US and Canadian ones must not keep the job open.
            "USCA; Remote - California",
            "Canada; Calgary; Winnipeg; Vancouver; Halifax; Edmonton; Ottawa; Toronto; Victoria; Montreal",
        ]);
    }

    #[test]
    fn other_continents_are_closed_by_country_and_city_too() {
        assert_closed(&[
            "Europe",
            "UK",
            "EMEA",
            "Remote France; Remote Germany; Remote UK",
            "Remote; Remote US",
            "Tokyo, Japan",
            "Remote - Japan",
            "Remote - Singapore",
            "Bangalore - Remote; India",
            "Sydney, Australia",
            "Dubai; United Arab Emirates",
            "Berlin",
        ]);
    }

    #[test]
    fn a_place_open_to_the_americas_or_latin_america_stays_open() {
        assert_open(&[
            "Remote - US, Canada or LATAM",
            "Remote - Europe or LATAM",
            "Northern America, LATAM, Europe, APAC",
            "Americas, Europe, Israel",
            "Worldwide",
            "Remote - Worldwide",
            "Anywhere",
            "Remote - Latin America",
            "Remote - Americas",
            "Remote - South America",
            "Santiago, Chile",
            "Remote - Brazil",
            "Remote, Colombia",
            "Bogotá",
            "São Paulo",
            "Mexico City, Mexico - Remote",
            "Remoto - Chile",
            // Remotive lists countries one by one: a Latin American one among them is enough.
            "USA, Canada, Argentina, Mexico, Peru",
        ]);
    }

    #[test]
    fn sales_style_region_codes_read_as_what_they_stand_for() {
        // Real locations from the replay: AMER is the Americas, the others are not.
        assert_open(&["Remote, AMER"]);
        assert_closed(&[
            "APJ",
            "Remote - NORAM",
            "Remote, KSA",
            "Remote - Luxembourg",
            "Remote - Cyprus",
            "Remote - DACH",
            // Working Nomads writes a Proxify posting's place as a time zone, 3 hours of slack at most.
            "CET (+/- 3 hours)",
            "Time zone: CET (+/- 3 hours)",
        ]);
    }

    #[test]
    fn anywhere_in_the_us_is_not_anywhere() {
        assert_closed(&[
            "Remote - Anywhere in the US",
            "Anywhere in the United States",
        ]);
    }

    #[test]
    fn a_word_that_merely_contains_a_country_code_is_no_country() {
        // `us` is inside "Mauritius", "Houstonian"-like words and `in` inside "Berlin" must not
        // be read as a state code either; unknown places are decided by the description.
        assert_open(&[
            "Remote - Mauritius",
            "Remote; Springfield",
            "Remote - Narnia",
        ]);
    }

    #[test]
    fn plain_remote_is_decided_by_the_description() {
        assert!(passes_saying(""));
        assert!(passes_saying("Build agents with Python and TypeScript."));
        assert!(passes("Engineer", "", true, ""));
    }

    #[test]
    fn a_description_that_confines_the_job_to_one_country_closes_it() {
        for sentence in [
            // From the first run's top 60.
            "Location: This role is remote (must be located in US East,Central, or West timezone)",
            "This role can be based remotely anywhere in the United States.",
            "U.S. citizenship is required due to Public Sector client-support requirements.",
            // The usual ways of saying it.
            "You must reside in the United States.",
            "Candidates must be authorized to work in the U.S.",
            "Applicants must be legally authorized to work in the United States.",
            "We are only hiring in the US.",
            "This is a US-based role.",
            "US-based candidates only.",
            "Must be US-based.",
            "Open to candidates located in the United States.",
            "We are unable to hire candidates outside the US.",
            "This position is remote within the US.",
            "Candidates must be located in Canada.",
            "You must be based in Europe.",
        ] {
            assert!(!passes_saying(sentence), "{sentence:?} confines the job");
        }
    }

    #[test]
    fn a_description_that_only_mentions_the_us_leaves_the_job_open() {
        for sentence in [
            // A real LatAm role and a real overlap requirement.
            "Based in Latin America with at least 6 hours of overlap with US Pacific time.",
            "Availability for significant daily overlap with US Eastern Time.",
            "You can work from Colombia, Argentina, Brazil, Chile, or anywhere in the Americas with strong overlap.",
            // About the company, the customers or the benefits, not about who may apply.
            "We are a US-based company hiring worldwide.",
            "Benefits for US-based employees include a 401(k).",
            "Our headquarters are in the United States.",
            "Compensation for US residents is $120,000 - $160,000.",
            // Real Wikimedia text (2026-10-04): a note for the US applicants of a global job.
            "For US-based applicants: this position is part of the Communication Workers of America bargaining unit.",
            "We welcome candidates outside the US.",
            "Must be fluent in English.",
            // A sentence that also names Latin America is not a restriction to the US.
            "You must be located in the US, Canada or LATAM.",
        ] {
            assert!(passes_saying(sentence), "{sentence:?} leaves the job open");
        }
    }

    #[test]
    fn a_us_location_wins_over_a_gentle_description() {
        assert!(!passes(
            "Engineer",
            "Remote - California",
            true,
            "Work from anywhere you like."
        ));
    }

    #[test]
    fn an_open_location_wins_over_a_us_sentence() {
        // The location is what the company chose to show; boilerplate about work authorization
        // must not close a job listed for the Americas.
        assert!(passes(
            "Engineer",
            "Remote - Americas",
            true,
            "Candidates must be authorized to work in the United States."
        ));
    }

    #[test]
    fn a_title_that_names_latam_keeps_a_remote_job_open() {
        // Fin's "Senior Solutions Engineer- LATAM" is listed for "NYC, Remote; San Francisco".
        let location = "NYC, Remote; San Francisco, California";
        assert!(passes(
            "Senior Solutions Engineer- LATAM",
            location,
            true,
            ""
        ));
        assert!(!passes("Senior Solutions Engineer", location, true, ""));
        assert!(!passes(
            "Senior Solutions Engineer- LATAM",
            location,
            false,
            ""
        ));
    }

    #[test]
    fn the_first_real_runs_top_scores_split_into_open_and_closed() {
        // (company, title, location as stored, stored remote flag, description sentence, from Chile?)
        // Hand-classified on 2026-10-04 from the top 60 of the database: 52 could not be done from
        // Chile, 4 could, 4 say nothing about it. Those said "remote" for every one of them.
        let rows = [
            (
                "Databricks",
                "Staff Security Software Engineer",
                "Remote - California",
                "This role can be based remotely anywhere in the United States.",
                false,
            ),
            (
                "Cohere",
                "Senior Full-Stack Engineer",
                "Toronto",
                "Cohere is remote-friendly, but we also have offices in Toronto, London, New York City.",
                false,
            ),
            (
                "Replit",
                "Staff Software Engineer, Agentic Ads",
                "Foster City, CA",
                "Commuter Benefits (In-Office & US Only)",
                false,
            ),
            (
                "Glean",
                "Resident Solutions Architect",
                "Remote",
                "This role is remote (must be located in US East, Central, or West timezone)",
                false,
            ),
            (
                "OpenAI",
                "Applied AI Architect, Retail",
                "San Francisco",
                "We use a hybrid work model of 3 days in the office per week.",
                false,
            ),
            (
                "Gusto",
                "Senior Staff Engineer",
                "Atlanta, GA - Hybrid; Austin, TX - Remote; Seattle, WA - Remote",
                "",
                false,
            ),
            (
                "Anthropic",
                "Staff+ Software Engineer",
                "Remote-Friendly (Travel Required) | San Francisco, CA",
                "",
                false,
            ),
            (
                "Databricks",
                "Director, Delivery Solutions Architects",
                "Maryland; Remote - Virginia; Remote - Washington D.C.",
                "U.S. citizenship is required.",
                false,
            ),
            (
                "OpenAI",
                "Applied AI Architect, Partner",
                "Tokyo, Japan",
                "",
                false,
            ),
            (
                "California Injury Group",
                "Senior Full-Stack Engineer",
                "Remote Remote",
                "Based in Latin America with at least 6 hours of overlap with US Pacific time.",
                true,
            ),
            (
                "Vequity",
                "Senior Full-Stack Engineer",
                "Remote Remote",
                "You can work from Colombia, Argentina, Brazil, Chile, or anywhere in the Americas with strong overlap.",
                true,
            ),
            (
                "Lemon.io",
                "Senior AI Engineer",
                "Northern America, LATAM, Europe, APAC",
                "",
                true,
            ),
            (
                "Mozilla",
                "Staff Software Engineer",
                "Remote",
                "This is a full-time, fully remote position.",
                true,
            ),
            (
                "Worthy",
                "Senior Full-Stack Engineer",
                "Remote Remote",
                "Ship features end to end across the stack.",
                true,
            ),
        ];
        for (company, title, location, description, from_chile) in rows {
            assert_eq!(
                passes(title, location, true, description),
                from_chile,
                "{company}: {title} ({location})"
            );
        }
    }

    /// The extra places `profile/criteria.toml` ships (the example profile is open to Australia).
    fn australia() -> Vec<String> {
        [
            "australia",
            "sydney",
            "melbourne",
            "brisbane",
            "perth",
            "adelaide",
            "canberra",
            "anz",
            "new south wales",
            "queensland",
        ]
        .map(String::from)
        .to_vec()
    }

    /// `passes` with the Australian extra places open.
    fn passes_in_australia(title: &str, location: &str, remote: bool, description: &str) -> bool {
        region_ok_with(
            &make_job_with(|job| {
                job.title = title.into();
                job.location = location.into();
                job.remote = remote;
                job.description = description.into();
            }),
            &australia(),
        )
    }

    #[test]
    fn australian_places_open_the_job_only_when_configured() {
        // Real locations from the 2026-10-21 probe of Ashby, Greenhouse and Lever boards.
        for (location, remote) in [
            ("Sydney, Australia", false),
            ("AU: Melbourne: (260 Burwood Rd)", false),
            ("AU - Sydney AU - Melbourne", false),
            ("Sydney Sydney Melbourne", false),
            ("Brisbane, Queensland", false),
            ("Perth, WA", true),
            ("Australia (Remote)", true),
            ("Remote - Australia", true),
            ("Remote, Australia", true),
            ("ANZ Region", true),
            ("London; Sydney", true),
            ("Remote - USA; Sydney, Australia", true),
        ] {
            assert!(
                passes_in_australia("Senior Software Engineer", location, remote, ""),
                "{location:?} is a job in Australia"
            );
            assert!(
                !super::region_ok(&make_job_with(|job| {
                    job.location = location.into();
                    job.remote = remote;
                })),
                "{location:?} stays closed without extra places"
            );
        }
    }

    #[test]
    fn the_extra_places_do_not_open_the_rest_of_the_world() {
        for (location, remote) in [
            ("Auckland, New Zealand", false),
            ("NZ: Auckland Remote", true),
            ("Remote - Singapore", true),
            ("London", false),
            ("Remote - USA", true),
            ("Remote, APAC", true),
            ("Tokyo, Japan", false),
        ] {
            assert!(
                !passes_in_australia("Senior Software Engineer", location, remote, ""),
                "{location:?} is not Australia"
            );
        }
        // A US-only sentence still closes a plain "Remote" job.
        assert!(!passes_in_australia(
            "Engineer",
            "Remote",
            true,
            "This is a US-based role."
        ));
    }

    #[test]
    fn a_job_in_australia_that_demands_work_rights_without_helping_is_closed() {
        for sentence in [
            // Real Harvey wording (2026-10-21).
            "Based in Sydney with valid Australian work rights.",
            "Work eligibility: Must have valid Australian work rights; Harvey does not currently offer visa sponsorship for this role.",
            "Based in Sydney and eligible to work in Australia.",
            "We do not offer visa sponsorship.",
            "Visa sponsorship is not available.",
            "We cannot sponsor visas for this role.",
            "Australian citizens only, due to the security clearance.",
            "You must have the right to work in Australia.",
            // Relocation help that is denied does not rescue it.
            "Must have the right to work in Australia. We do not offer relocation assistance.",
        ] {
            for (location, remote) in [("Sydney, Australia", false), ("Australia (Remote)", true)] {
                assert!(
                    !passes_in_australia("Engineer", location, remote, sentence),
                    "{location:?} with {sentence:?}"
                );
            }
        }
    }

    #[test]
    fn an_offer_of_sponsorship_or_relocation_keeps_the_job_in_australia() {
        for sentence in [
            // Real Anthropic and OpenAI wording (2026-10-21).
            "Visa sponsorship: We do sponsor visas! However, we aren't able to successfully sponsor visas for every role and every candidate.",
            "We use a hybrid work model of 3 days in the office per week. We offer relocation assistance.",
            "Relocation assistance is available to new employees.",
            // Silence about visas is no restriction: most postings say nothing.
            "Build agents with Python and TypeScript.",
            "",
            // Even next to a requirement, an offer rescues the posting.
            "Must have the right to work in Australia. We can sponsor visas for strong candidates.",
        ] {
            assert!(
                passes_in_australia("Engineer", "Sydney, Australia", false, sentence),
                "{sentence:?}"
            );
        }
    }

    #[test]
    fn no_sponsorship_only_matters_for_a_job_that_is_in_australia() {
        // A remote contractor job open to Chile or the Americas, or one that names no place, is
        // done from Chile: "no visa sponsorship" says nothing about it.
        for location in ["Remote - LATAM", "Remote - Americas", "Worldwide", "Remote"] {
            assert!(
                passes_in_australia(
                    "Engineer",
                    location,
                    true,
                    "We do not offer visa sponsorship."
                ),
                "{location:?}"
            );
        }
    }

    #[test]
    fn a_description_that_confines_the_job_to_australia_makes_it_a_job_there() {
        let confines = "Candidates must be located in Australia.";
        assert!(!passes_saying(confines), "closed without extra places");
        assert!(passes_in_australia("Engineer", "Remote", true, confines));
        assert!(!passes_in_australia(
            "Engineer",
            "Remote",
            true,
            "Candidates must be located in Australia. We do not offer visa sponsorship."
        ));
    }

    #[test]
    fn blank_extra_places_change_nothing() {
        let job = make_job_with(|job| {
            job.location = "Sydney, Australia".into();
            job.remote = false;
        });
        assert!(!region_ok_with(&job, &[String::new(), "  ".to_string()]));
    }

    #[test]
    fn a_job_that_is_not_remote_needs_a_chilean_place() {
        assert!(passes("Engineer", "Santiago, Chile", false, ""));
        assert!(passes("Engineer", "Las Condes; Santiago", false, ""));
        assert!(!passes("Engineer", "Bogotá, Colombia", false, ""));
        assert!(!passes("Engineer", "San Francisco", false, ""));
        assert!(!passes("Engineer", "", false, ""));
    }
}
