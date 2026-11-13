//! APA 7 reference-list entries — the deterministic formatter behind
//! `cite_sources`.
//!
//! Rule source (operator ruling 2026-11-12: the tool emits APA): Purdue
//! OWL's APA 7 pages "Reference List: Electronic Sources", "Reference
//! List: Author/Authors" and "Reference List: Basic Rules", read
//! 2026-11-12. OWL's own examples are this module's fixtures.
//!
//! What a formatter cannot decide is left to the caller and stated in the
//! tool description: titles are emitted as supplied (APA's sentence case
//! needs proper-noun knowledge), and an author is a person only when the
//! caller gives a family name.
//!
//! Three choices here are not stated on those pages: a hyphenated given
//! name keeps its hyphen in the initials (`Jean-Paul` → `J.-P.`), no
//! period is added after a title that already ends in `.`, `?` or `!`, and
//! italics are written as markdown asterisks.

use schemars::{JsonSchema, json_schema};
use serde::Deserialize;

/// One author. A string is a group or organization name, emitted as
/// written. An object is a person, emitted inverted with initials.
#[derive(Debug, Clone)]
pub enum CiteAuthor {
    Group(String),
    Person(PersonName),
}

// Hand-written so a malformed author is refused with a message that says
// what an author is. The derived untagged form answers every mistake with
// "data did not match any variant", which names neither the field nor the
// accepted shapes.
impl<'de> Deserialize<'de> for CiteAuthor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AuthorVisitor;
        impl<'de> serde::de::Visitor<'de> for AuthorVisitor {
            type Value = CiteAuthor;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(
                    "an author: a string (a group or organization name) or an object {family, given} (a person)",
                )
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<CiteAuthor, E> {
                Ok(CiteAuthor::Group(value.to_string()))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<CiteAuthor, A::Error> {
                PersonName::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(CiteAuthor::Person)
            }
        }
        deserializer.deserialize_any(AuthorVisitor)
    }
}

/// A person's name. `family` is required; `given` holds the first and
/// middle names (or initials) and may be omitted.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PersonName {
    /// Family (last) name, e.g. "Cormack" or "van Gogh".
    pub family: String,
    /// Given and middle names or initials, e.g. "Gordon V." Emitted as initials.
    pub given: Option<String>,
}

impl JsonSchema for CiteAuthor {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CiteAuthor".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        json_schema!({
            "description": "An author. A string is a group or organization name, emitted as written (e.g. \"World Health Organization\"). An object is a person, emitted as \"Family, G. M.\".",
            "anyOf": [
                {"type": "string"},
                {
                    "type": "object",
                    "properties": {
                        "family": {"type": "string", "description": "Family (last) name."},
                        "given": {"type": ["string", "null"], "description": "Given and middle names or initials."}
                    },
                    "required": ["family"],
                    "additionalProperties": false
                }
            ]
        })
    }
}

/// One source to cite.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CiteSource {
    /// Where the work is retrieved from. Required unless `doi` is given.
    pub url: Option<String>,
    /// The work's title, as it should appear. APA uses sentence case for
    /// page and article titles; the tool does not re-case a title.
    pub title: String,
    /// Authors in order. Omit or pass an empty list when the work names no
    /// author: the title then takes the author position.
    pub authors: Option<Vec<CiteAuthor>>,
    /// Publication date: `YYYY`, `YYYY-MM`, `YYYY-MM-DD` or an RFC 3339
    /// timestamp. Omit when the work carries no date: the entry reads
    /// `(n.d.)`. Any other form is rejected.
    pub published: Option<String>,
    /// Name of the website for a web page (e.g. "Medium"). Left out of the
    /// entry when it equals the single group author. For a journal article
    /// pass `journal` instead.
    pub site_name: Option<String>,
    /// Journal title for an article in a periodical, as the journal writes it.
    pub journal: Option<String>,
    /// Journal volume.
    pub volume: Option<String>,
    /// Journal issue.
    pub issue: Option<String>,
    /// Page range or article number.
    pub pages: Option<String>,
    /// DOI in any form (bare, `doi:`, or a doi.org URL). When present the
    /// entry ends with `https://doi.org/…` instead of `url`.
    pub doi: Option<String>,
    /// The date the page was read, `YYYY-MM-DD`. Give it for content that
    /// changes over time; the entry then ends "Retrieved Month D, YYYY, from URL".
    pub retrieved: Option<String>,
}

/// The parsed publication date, at the precision the caller supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PublishedDate {
    year: i32,
    month: Option<u32>,
    day: Option<u32>,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn parse_ymd(text: &str, field: &str, index: usize) -> Result<PublishedDate, String> {
    let reject = || {
        format!(
            "sources[{index}].{field} must be YYYY, YYYY-MM, YYYY-MM-DD or an RFC 3339 timestamp, got '{text}'"
        )
    };
    let date_part = text.split(['T', 't', ' ']).next().unwrap_or(text);
    if date_part.len() != text.len() && chrono::DateTime::parse_from_rfc3339(text).is_err() {
        return Err(reject());
    }
    let parts: Vec<&str> = date_part.split('-').collect();
    let all_digits = |part: &str, len: usize| {
        part.len() == len && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    match parts.as_slice() {
        [year] if all_digits(year, 4) => Ok(PublishedDate {
            year: year.parse().map_err(|_| reject())?,
            month: None,
            day: None,
        }),
        [year, month] if all_digits(year, 4) && all_digits(month, 2) => {
            let month: u32 = month.parse().map_err(|_| reject())?;
            if !(1..=12).contains(&month) {
                return Err(reject());
            }
            Ok(PublishedDate {
                year: year.parse().map_err(|_| reject())?,
                month: Some(month),
                day: None,
            })
        }
        [year, month, day] if all_digits(year, 4) && all_digits(month, 2) && all_digits(day, 2) => {
            let date =
                chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").map_err(|_| reject())?;
            use chrono::Datelike;
            Ok(PublishedDate {
                year: date.year(),
                month: Some(date.month()),
                day: Some(date.day()),
            })
        }
        _ => Err(reject()),
    }
}

/// `(2018, March 23)`, `(2010, July)`, `(2019)` or `(n.d.)`, with the
/// same-author-same-year letter suffix when one was assigned.
fn date_element(date: Option<PublishedDate>, suffix: Option<char>) -> String {
    let Some(date) = date else {
        return "(n.d.)".to_string();
    };
    let suffix = suffix.map(String::from).unwrap_or_default();
    match (date.month, date.day) {
        (Some(month), Some(day)) => format!(
            "({}{suffix}, {} {day})",
            date.year,
            MONTHS[(month - 1) as usize]
        ),
        (Some(month), None) => format!("({}{suffix}, {})", date.year, MONTHS[(month - 1) as usize]),
        _ => format!("({}{suffix})", date.year),
    }
}

/// Initials of a given-name string: `Gordon V.` → `G. V.`, `Jean-Paul` →
/// `J.-P.`, `D` → `D.`.
fn initials(given: &str) -> String {
    given
        .split_whitespace()
        .map(|word| {
            word.split('-')
                .filter_map(|piece| piece.chars().find(|character| character.is_alphabetic()))
                .map(|initial| format!("{}.", initial.to_uppercase()))
                .collect::<Vec<_>>()
                .join("-")
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn author_text(author: &CiteAuthor) -> String {
    match author {
        CiteAuthor::Group(name) => name.trim().to_string(),
        CiteAuthor::Person(person) => {
            let family = person.family.trim();
            match person.given.as_deref().map(initials) {
                Some(given) if !given.is_empty() => format!("{family}, {given}"),
                _ => family.to_string(),
            }
        }
    }
}

/// The author element. One author stands alone; two to twenty are joined
/// by commas with an ampersand before the last; twenty-one or more list
/// the first nineteen, an ellipsis, and the final author with no ampersand.
fn author_element(authors: &[CiteAuthor]) -> String {
    let names: Vec<String> = authors.iter().map(author_text).collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        2..=20 => {
            let (last, rest) = names.split_last().expect("at least two names");
            format!("{}, & {last}", rest.join(", "))
        }
        _ => format!(
            "{}, . . . {}",
            names[..19].join(", "),
            names.last().expect("more than twenty names")
        ),
    }
}

/// End an element with a period unless it already ends a sentence.
fn closed(element: &str) -> String {
    let trimmed = element.trim();
    if trimmed.ends_with(['.', '?', '!']) {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

fn italic(text: &str) -> String {
    format!("*{}*", text.trim())
}

/// The DOI as a doi.org URL, from any of its written forms.
fn doi_url(doi: &str, index: usize) -> Result<String, String> {
    let lowered = doi.trim().to_lowercase();
    let bare = [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi:",
    ]
    .iter()
    .find_map(|prefix| lowered.strip_prefix(prefix))
    .unwrap_or(&lowered)
    .trim();
    if !bare.starts_with("10.") || !bare.contains('/') {
        return Err(format!(
            "sources[{index}].doi must be a DOI (10.…/…), bare or as a doi.org URL, got '{doi}'"
        ));
    }
    Ok(format!("https://doi.org/{bare}"))
}

/// A validated source: every field the entry needs, parsed.
struct Prepared {
    index: usize,
    authors: Vec<CiteAuthor>,
    author_element: String,
    date: Option<PublishedDate>,
    title: String,
    site_name: Option<String>,
    journal: Option<String>,
    volume: Option<String>,
    issue: Option<String>,
    pages: Option<String>,
    locator: String,
    retrieved: Option<PublishedDate>,
}

fn present(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn prepare(index: usize, source: &CiteSource) -> Result<Prepared, String> {
    let title = source.title.trim();
    if title.is_empty() {
        return Err(format!("sources[{index}].title must not be empty"));
    }
    let authors: Vec<CiteAuthor> = source.authors.clone().unwrap_or_default();
    for (position, author) in authors.iter().enumerate() {
        if author_text(author).is_empty() {
            return Err(format!(
                "sources[{index}].authors[{position}] must not be empty"
            ));
        }
    }
    let date = match present(&source.published) {
        Some(text) => Some(parse_ymd(&text, "published", index)?),
        None => None,
    };
    let retrieved = match present(&source.retrieved) {
        Some(text) => {
            let parsed = parse_ymd(&text, "retrieved", index)?;
            if parsed.day.is_none() {
                return Err(format!(
                    "sources[{index}].retrieved must be a full date YYYY-MM-DD, got '{text}'"
                ));
            }
            Some(parsed)
        }
        None => None,
    };
    let locator = match (present(&source.doi), present(&source.url)) {
        (Some(doi), _) => doi_url(&doi, index)?,
        (None, Some(url)) => url,
        (None, None) => {
            return Err(format!("sources[{index}] needs a url or a doi"));
        }
    };
    let journal = present(&source.journal);
    for (field, value) in [
        ("volume", &source.volume),
        ("issue", &source.issue),
        ("pages", &source.pages),
    ] {
        if present(value).is_some() && journal.is_none() {
            return Err(format!(
                "sources[{index}].{field} is a journal field: pass `journal` as well"
            ));
        }
    }
    if journal.is_some() && present(&source.site_name).is_some() {
        return Err(format!(
            "sources[{index}] has both journal and site_name: a journal article takes `journal`, a web page takes `site_name`"
        ));
    }
    Ok(Prepared {
        index,
        author_element: author_element(&authors),
        authors,
        date,
        title: title.to_string(),
        site_name: present(&source.site_name),
        journal,
        volume: present(&source.volume),
        issue: present(&source.issue),
        pages: present(&source.pages),
        locator,
        retrieved,
    })
}

/// One formatted entry and the position of its source in the request.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Reference {
    /// Index of the source in the request's `sources`.
    pub source_index: usize,
    /// The reference-list entry. Italics are markdown asterisks.
    pub reference: String,
}

/// The formatted list and what the formatter could not settle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceList {
    pub references: Vec<Reference>,
    pub notes: Vec<String>,
}

fn entry(source: &Prepared, suffix: Option<char>) -> String {
    let date = date_element(source.date, suffix);
    let mut parts: Vec<String> = Vec::new();
    let is_article = source.journal.is_some();
    // A web page's title is italic; a journal article's is plain.
    let title = if is_article {
        closed(&source.title)
    } else {
        let trimmed = source.title.trim();
        if trimmed.ends_with(['.', '?', '!']) {
            italic(trimmed)
        } else {
            format!("{}.", italic(trimmed))
        }
    };
    if source.author_element.is_empty() {
        // No author: the title takes the author position.
        parts.push(title);
        parts.push(format!("{date}."));
    } else {
        parts.push(closed(&source.author_element));
        parts.push(format!("{date}."));
        parts.push(title);
    }
    if let Some(journal) = &source.journal {
        // *Journal, 23*(2), 161-180.
        let mut periodical = match &source.volume {
            Some(volume) => format!("*{journal}, {volume}*"),
            None => italic(journal),
        };
        if let (Some(issue), Some(_)) = (&source.issue, &source.volume) {
            periodical.push_str(&format!("({issue})"));
        }
        if let Some(pages) = &source.pages {
            periodical.push_str(&format!(", {pages}"));
        }
        parts.push(format!("{periodical}."));
    } else if let Some(site) = &source.site_name {
        // The site name is omitted when it is the same as the author.
        let same_as_author = match source.authors.as_slice() {
            [CiteAuthor::Group(group)] => group.trim().eq_ignore_ascii_case(site),
            _ => false,
        };
        if !same_as_author {
            parts.push(closed(site));
        }
    }
    match source.retrieved {
        Some(retrieved) => parts.push(format!(
            "Retrieved {} {}, {}, from {}",
            MONTHS[(retrieved.month.unwrap_or(1) - 1) as usize],
            retrieved.day.unwrap_or(1),
            retrieved.year,
            source.locator
        )),
        None => parts.push(source.locator.clone()),
    }
    parts.join(" ")
}

/// The key an entry is alphabetized by: the author element, or the title
/// when the work names no author, compared without case.
fn sort_key(source: &Prepared) -> String {
    if source.author_element.is_empty() {
        source.title.to_lowercase()
    } else {
        source.author_element.to_lowercase()
    }
}

/// Format `sources` as an APA 7 reference list, in reference-list order.
///
/// Every rejection names the source index and the field. Entries are
/// alphabetized by first author (by title when a work names no author);
/// works by the same author(s) run earliest first with undated works
/// before dated ones; works by the same author(s) in the same year run
/// year-only before more specific dates, then by title, and take letter
/// suffixes.
pub fn reference_list(sources: &[CiteSource]) -> Result<ReferenceList, String> {
    let mut prepared: Vec<Prepared> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| prepare(index, source))
        .collect::<Result<_, _>>()?;
    prepared.sort_by(|left, right| {
        sort_key(left)
            .cmp(&sort_key(right))
            .then_with(|| {
                left.date
                    .map(|date| date.year)
                    .cmp(&right.date.map(|date| date.year))
            })
            .then_with(|| left.date.cmp(&right.date))
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.index.cmp(&right.index))
    });

    // Letter suffixes: runs of entries with the same author element and year.
    let mut suffixes: Vec<Option<char>> = vec![None; prepared.len()];
    let mut notes: Vec<String> = Vec::new();
    let mut start = 0;
    while start < prepared.len() {
        let same_group = |candidate: &Prepared| {
            !prepared[start].author_element.is_empty()
                && candidate
                    .author_element
                    .eq_ignore_ascii_case(&prepared[start].author_element)
                && candidate.date.map(|date| date.year)
                    == prepared[start].date.map(|date| date.year)
        };
        let end = (start + 1..prepared.len())
            .find(|&position| !same_group(&prepared[position]))
            .unwrap_or(prepared.len());
        if end - start > 1 {
            let indices: Vec<usize> = prepared[start..end]
                .iter()
                .map(|source| source.index)
                .collect();
            if prepared[start].date.is_none() {
                notes.push(format!(
                    "sources {indices:?} share an author and carry no date: they are listed by title and are not distinguished by a letter suffix"
                ));
            } else if end - start > 26 {
                notes.push(format!(
                    "sources {indices:?} share an author and a year: more than 26, so no letter suffixes were assigned"
                ));
            } else {
                for (offset, slot) in suffixes[start..end].iter_mut().enumerate() {
                    *slot = Some((b'a' + offset as u8) as char);
                }
            }
        }
        start = end;
    }

    let references = prepared
        .iter()
        .zip(suffixes)
        .map(|(source, suffix)| Reference {
            source_index: source.index,
            reference: entry(source, suffix),
        })
        .collect();
    Ok(ReferenceList { references, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(family: &str, given: &str) -> CiteAuthor {
        CiteAuthor::Person(PersonName {
            family: family.to_string(),
            given: Some(given.to_string()),
        })
    }

    fn group(name: &str) -> CiteAuthor {
        CiteAuthor::Group(name.to_string())
    }

    fn page(title: &str, url: &str) -> CiteSource {
        CiteSource {
            url: Some(url.to_string()),
            title: title.to_string(),
            authors: None,
            published: None,
            site_name: None,
            journal: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
            retrieved: None,
        }
    }

    fn one(source: CiteSource) -> String {
        reference_list(&[source]).expect("formats").references[0]
            .reference
            .clone()
    }

    /// OWL, "Webpage or Piece of Online Content", individual author.
    #[test]
    fn webpage_with_an_individual_author() {
        let source = CiteSource {
            authors: Some(vec![person("Price", "Devon")]),
            published: Some("2018-03-23".to_string()),
            site_name: Some("Medium".to_string()),
            ..page(
                "Laziness does not exist",
                "https://humanparts.medium.com/laziness-does-not-exist-3af27e312d01",
            )
        };
        assert_eq!(
            one(source),
            "Price, D. (2018, March 23). *Laziness does not exist*. Medium. https://humanparts.medium.com/laziness-does-not-exist-3af27e312d01"
        );
    }

    /// OWL: a group author, and the site name omitted when it is the author.
    #[test]
    fn group_author_omits_a_site_name_equal_to_the_author() {
        let source = CiteSource {
            authors: Some(vec![group(
                "American Society for the Prevention of Cruelty to Animals",
            )]),
            published: Some("2019-11-21".to_string()),
            site_name: Some(
                "American Society for the Prevention of Cruelty to Animals".to_string(),
            ),
            ..page(
                "Justice served: Case closed for over 40 dogfighting victims",
                "https://www.aspca.org/news/justice-served-case-closed-over-40-dogfighting-victims",
            )
        };
        assert_eq!(
            one(source),
            "American Society for the Prevention of Cruelty to Animals. (2019, November 21). *Justice served: Case closed for over 40 dogfighting victims*. https://www.aspca.org/news/justice-served-case-closed-over-40-dogfighting-victims"
        );
    }

    /// OWL: no author — the title leads; a retrieval date for changing content.
    #[test]
    fn no_author_moves_the_title_first_and_a_retrieval_date_is_written_out() {
        let source = CiteSource {
            published: Some("2026-04-23".to_string()),
            site_name: Some("Purdue OWL".to_string()),
            retrieved: Some("2026-06-01".to_string()),
            ..page(
                "Reference list: Electronic sources",
                "https://owl.purdue.edu/owl/research_and_citation/apa_style/apa_formatting_and_style_guide/reference_list_electronic_sources.html",
            )
        };
        assert_eq!(
            one(source),
            "*Reference list: Electronic sources*. (2026, April 23). Purdue OWL. Retrieved June 1, 2026, from https://owl.purdue.edu/owl/research_and_citation/apa_style/apa_formatting_and_style_guide/reference_list_electronic_sources.html"
        );
    }

    /// OWL: no date — `(n.d.)`.
    #[test]
    fn no_date_reads_n_d() {
        let source = CiteSource {
            authors: Some(vec![group("National Alliance on Mental Illness")]),
            ..page(
                "Mental health conditions",
                "https://www.nami.org/Learn-More/Mental-Health-Conditions",
            )
        };
        assert_eq!(
            one(source),
            "National Alliance on Mental Illness. (n.d.). *Mental health conditions*. https://www.nami.org/Learn-More/Mental-Health-Conditions"
        );
    }

    /// OWL, "Article from an Online Periodical with DOI Assigned".
    #[test]
    fn journal_article_with_a_doi() {
        let source = CiteSource {
            url: None,
            authors: Some(vec![
                person("Drollinger", "Tanya"),
                person("Comer", "Lucette B."),
                person("Warrington", "Patricia T."),
            ]),
            published: Some("2006".to_string()),
            journal: Some("Psychology & Marketing".to_string()),
            volume: Some("23".to_string()),
            issue: Some("2".to_string()),
            pages: Some("161-180".to_string()),
            doi: Some("10.1002/mar.20105".to_string()),
            ..page(
                "Development and validation of the active empathetic listening scale",
                "unused",
            )
        };
        assert_eq!(
            one(source),
            "Drollinger, T., Comer, L. B., & Warrington, P. T. (2006). Development and validation of the active empathetic listening scale. *Psychology & Marketing, 23*(2), 161-180. https://doi.org/10.1002/mar.20105"
        );
    }

    /// OWL, "Two Authors": a comma and an ampersand.
    #[test]
    fn two_authors_are_joined_by_a_comma_and_an_ampersand() {
        assert_eq!(
            author_element(&[
                person("Soto", "Christopher J."),
                person("John", "Oliver P.")
            ]),
            "Soto, C. J., & John, O. P."
        );
    }

    /// OWL, "More Than Twenty Authors": nineteen, an ellipsis, the last.
    #[test]
    fn twenty_one_authors_list_nineteen_an_ellipsis_and_the_last() {
        let authors: Vec<CiteAuthor> = (1..=21)
            .map(|number| person(&format!("Family{number}"), "A"))
            .collect();
        let element = author_element(&authors);
        assert!(
            element.starts_with("Family1, A., Family2, A.,"),
            "{element}"
        );
        assert!(
            element.ends_with("Family19, A., . . . Family21, A."),
            "{element}"
        );
        assert!(!element.contains("Family20"), "{element}");
        assert!(!element.contains('&'), "{element}");

        let twenty: Vec<CiteAuthor> = authors[..20].to_vec();
        let element = author_element(&twenty);
        assert!(
            element.ends_with("Family19, A., & Family20, A."),
            "{element}"
        );
    }

    /// OWL, "Two or More Works by the Same Author" and "...in the Same Year".
    #[test]
    fn the_list_is_ordered_and_same_year_works_take_letter_suffixes() {
        let work = |title: &str, published: Option<&str>| CiteSource {
            authors: Some(vec![person("Berndt", "Thomas J.")]),
            published: published.map(str::to_string),
            ..page(title, "https://example.org/x")
        };
        let list = reference_list(&[
            work("Friendship and three A's", Some("2004")),
            work("Later work", Some("2011")),
            work("Children's friendships", Some("2004")),
            work("Undated work", None),
            CiteSource {
                authors: Some(vec![person("Agnew", "Christopher R.")]),
                published: Some("2014".to_string()),
                ..page("Social influences", "https://example.org/a")
            },
        ])
        .expect("formats");
        let order: Vec<usize> = list
            .references
            .iter()
            .map(|entry| entry.source_index)
            .collect();
        assert_eq!(order, vec![4, 3, 2, 0, 1], "{list:?}");
        assert!(list.references[1].reference.contains("(n.d.)"));
        assert!(list.references[2].reference.contains("(2004a)"));
        assert!(list.references[3].reference.contains("(2004b)"));
        assert!(list.references[4].reference.contains("(2011)"));
        assert!(list.notes.is_empty(), "{:?}", list.notes);
    }

    #[test]
    fn a_date_that_is_not_a_date_is_refused_naming_the_source_and_the_forms() {
        let error = reference_list(&[CiteSource {
            published: Some("3 days ago".to_string()),
            ..page("T", "https://example.org/")
        }])
        .expect_err("refused");
        assert!(error.contains("sources[0].published"), "{error}");
        assert!(error.contains("YYYY-MM-DD"), "{error}");
        assert!(error.contains("3 days ago"), "{error}");
    }

    #[test]
    fn dates_keep_the_precision_they_were_given() {
        assert_eq!(
            date_element(Some(parse_ymd("2010-07", "published", 0).unwrap()), None),
            "(2010, July)"
        );
        assert_eq!(
            date_element(
                Some(parse_ymd("2024-11-30T10:00:00Z", "published", 0).unwrap()),
                None
            ),
            "(2024, November 30)"
        );
        assert!(parse_ymd("2024-13", "published", 0).is_err());
        assert!(parse_ymd("2024-02-30", "published", 0).is_err());
        assert!(parse_ymd("24", "published", 0).is_err());
    }

    #[test]
    fn initials_handle_hyphens_existing_initials_and_missing_given_names() {
        assert_eq!(initials("Jean-Paul"), "J.-P.");
        assert_eq!(initials("G. V."), "G. V.");
        assert_eq!(initials("gordon"), "G.");
        assert_eq!(
            author_text(&CiteAuthor::Person(PersonName {
                family: "Plato".to_string(),
                given: None
            })),
            "Plato"
        );
    }

    #[test]
    fn a_doi_is_written_as_a_doi_org_url_from_any_form() {
        assert_eq!(
            doi_url("DOI:10.1145/1571941.1572114", 0).unwrap(),
            "https://doi.org/10.1145/1571941.1572114"
        );
        assert_eq!(
            doi_url("https://dx.doi.org/10.1037/pspp0000096", 0).unwrap(),
            "https://doi.org/10.1037/pspp0000096"
        );
        assert!(doi_url("not-a-doi", 0).is_err());
    }

    #[test]
    fn a_source_needs_a_title_and_a_locator_and_journal_fields_need_a_journal() {
        let no_locator = CiteSource {
            url: None,
            ..page("T", "unused")
        };
        assert!(
            reference_list(&[no_locator])
                .unwrap_err()
                .contains("needs a url or a doi")
        );
        assert!(
            reference_list(&[page("  ", "https://example.org/")])
                .unwrap_err()
                .contains("title must not be empty")
        );
        let stray_volume = CiteSource {
            volume: Some("3".to_string()),
            ..page("T", "https://example.org/")
        };
        assert!(
            reference_list(&[stray_volume])
                .unwrap_err()
                .contains("pass `journal`")
        );
    }

    /// A malformed author is refused saying what an author is, and a
    /// person object names its unknown key.
    #[test]
    fn a_malformed_author_is_refused_with_the_accepted_shapes() {
        let number = serde_json::from_value::<CiteAuthor>(serde_json::json!(7))
            .expect_err("a number is not an author")
            .to_string();
        assert!(
            number.contains("a string (a group or organization name)"),
            "{number}"
        );
        assert!(number.contains("{family, given}"), "{number}");

        let misspelled =
            serde_json::from_value::<CiteAuthor>(serde_json::json!({"family": "A", "givn": "B"}))
                .expect_err("unknown key")
                .to_string();
        assert!(misspelled.contains("givn"), "{misspelled}");

        assert!(matches!(
            serde_json::from_value::<CiteAuthor>(serde_json::json!("World Health Organization")),
            Ok(CiteAuthor::Group(name)) if name == "World Health Organization"
        ));
        assert!(matches!(
            serde_json::from_value::<CiteAuthor>(serde_json::json!({"family": "Price"})),
            Ok(CiteAuthor::Person(person)) if person.family == "Price" && person.given.is_none()
        ));
    }

    #[test]
    fn a_title_that_ends_a_sentence_gets_no_second_period() {
        assert_eq!(
            one(page("Why?", "https://example.org/")),
            "*Why?* (n.d.). https://example.org/"
        );
    }
}
