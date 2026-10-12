//! repozier — wypisuje link do losowego repozytorium z GitHuba.

use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Górna granica ID repozytoriów. ID rosną z czasem, więc od czasu do czasu
/// warto tę wartość podnieść. Jeśli losowe `since` wypadnie powyżej
/// aktualnego maksimum, program sam zmniejszy zakres i spróbuje ponownie.
const DEFAULT_MAX_ID: u64 = 1_400_000_000;
/// Ile repozytoriów GitHub zwraca na jedną stronę (maksymalnie 100).
const PAGE_SIZE: u32 = 100;
/// Rozmiar strony w wyszukiwarce GitHuba (maksymalnie 100).
const SEARCH_PAGE_SIZE: u32 = 100;
/// Ile razy próbować na jedno repozytorium, zanim uznamy filtry za zbyt wąskie.
const MAX_ATTEMPTS_PER_REPO: usize = 20;
/// Wyszukiwarka GitHuba udostępnia tylko pierwsze 1000 wyników.
const SEARCH_MAX_RESULTS: u64 = 1000;
/// Maksymalny numer strony w wyszukiwarce przy `SEARCH_PAGE_SIZE`.
const SEARCH_MAX_PAGES: u64 = SEARCH_MAX_RESULTS / SEARCH_PAGE_SIZE as u64;
/// Maksymalna liczba zapytań do wyszukiwarki na jedno uruchomienie.
const SEARCH_MAX_REQUESTS: usize = 8;

#[derive(Parser, Debug)]
#[command(
    name = "repozier",
    version,
    about = "Losowe repozytorium z GitHuba prosto z terminala",
    after_help = "Bez tokenu obowiązuje limit ~60 zapytań/h (wyszukiwarka: ~10/min).\nUstaw GITHUB_TOKEN, aby podnieść limity.\n\nZawsze wybierane jest losowe repozytorium spełniające podane filtry."
)]
struct Cli {
    /// Ile repozytoriów wypisać
    #[arg(short = 'n', long, default_value_t = 1)]
    count: usize,

    /// Filtruj po języku programowania (np. rust, python, go)
    #[arg(short = 'l', long, value_name = "JEZYK")]
    language: Option<String>,

    /// Tylko repozytoria użytkownika lub organizacji
    #[arg(short = 'u', long, value_name = "UZYTKOWNIK")]
    user: Option<String>,

    /// Tylko repozytoria organizacji
    #[arg(short = 'o', long, value_name = "ORGANIZACJA")]
    org: Option<String>,

    /// Wyklucz repozytoria danego użytkownika (można podać wielokrotnie)
    #[arg(long, value_name = "UZYTKOWNIK")]
    exclude_user: Option<String>,

    /// Tylko repozytoria z danym tematem (można podać wielokrotnie)
    #[arg(short = 't', long = "topic", value_name = "TEMAT")]
    topic: Vec<String>,

    /// Tylko repozytoria, których nazwa zawiera podany tekst
    #[arg(long, value_name = "TEKST")]
    name: Option<String>,

    /// Tylko repozytoria na danej licencji (np. mit, apache-2.0)
    #[arg(long, value_name = "LICENCJA")]
    license: Option<String>,

    /// Minimalna liczba gwiazdek
    #[arg(long, default_value_t = 0, value_name = "N")]
    min_stars: u64,

    /// Maksymalna liczba gwiazdek
    #[arg(long, value_name = "N")]
    max_stars: Option<u64>,

    /// Minimalna liczba forków
    #[arg(long, value_name = "N")]
    min_forks: Option<u64>,

    /// Maksymalna liczba forków
    #[arg(long, value_name = "N")]
    max_forks: Option<u64>,

    /// Minimalny rozmiar repozytorium w kilobajtach
    #[arg(long, value_name = "KB")]
    min_size: Option<u64>,

    /// Maksymalny rozmiar repozytorium w kilobajtach
    #[arg(long, value_name = "KB")]
    max_size: Option<u64>,

    /// Tylko repozytoria utworzone po dacie (YYYY-MM-DD)
    #[arg(long, value_name = "DATA")]
    created_after: Option<String>,

    /// Tylko repozytoria utworzone przed datą (YYYY-MM-DD)
    #[arg(long, value_name = "DATA")]
    created_before: Option<String>,

    /// Tylko repozytoria z ostatnim pushem po dacie (YYYY-MM-DD)
    #[arg(long, value_name = "DATA")]
    pushed_after: Option<String>,

    /// Pomijaj forki
    #[arg(long)]
    no_forks: bool,

    /// Pomijaj zarchiwizowane repozytoria
    #[arg(long)]
    no_archived: bool,

    /// Pomijaj puste repozytoria (bez żadnej zawartości)
    #[arg(long)]
    no_empty: bool,

    /// Tylko repozytoria z opisem
    #[arg(long)]
    has_description: bool,

    /// Tylko repozytoria będące mirrorami
    #[arg(long)]
    mirror: bool,

    /// Pomijaj mirrory
    #[arg(long)]
    no_mirror: bool,

    /// Tylko repozytoria szablonowe (template)
    #[arg(long)]
    template: bool,

    /// Pomijaj repozytoria szablonowe (template)
    #[arg(long)]
    no_template: bool,

    /// Tylko repozytoria z włączonymi Issues
    #[arg(long)]
    has_issues: bool,

    /// Tylko repozytoria z wyłączonymi Issues
    #[arg(long)]
    no_issues: bool,

    /// Tylko repozytoria z włączonym wiki
    #[arg(long)]
    has_wiki: bool,

    /// Tylko repozytoria z wyłączonym wiki
    #[arg(long)]
    no_wiki: bool,

    /// Tylko repozytoria z włączonym GitHub Pages
    #[arg(long)]
    has_pages: bool,

    /// Tylko repozytoria z wyłączonym GitHub Pages
    #[arg(long)]
    no_pages: bool,

    /// Tylko repozytoria z daną domyślną gałęzią
    #[arg(long, value_name = "NAZWA")]
    default_branch: Option<String>,

    /// Wypisz wynik jako JSON
    #[arg(long)]
    json: bool,

    /// Otwórz link w przeglądarce
    #[arg(long)]
    open: bool,

    /// Górna granica losowanego ID (zaawansowane, tylko bez filtrów)
    #[arg(long, value_name = "ID")]
    max_id: Option<u64>,

    /// Token GitHub (domyślnie brany ze zmiennej GITHUB_TOKEN)
    #[arg(long, env = "GITHUB_TOKEN", hide_env_values = true, value_name = "TOKEN")]
    token: Option<String>,
}

impl Cli {
    /// Czy podano jakikolwiek filtr treści? Jeśli tak, losujemy przez
    /// wyszukiwarkę GitHuba (obsługuje filtry serwerowo). W przeciwnym razie
    /// skanujemy ID, co daje równomierny rozkład po całym GitHubie.
    fn has_filters(&self) -> bool {
        self.language.is_some()
            || self.user.is_some()
            || self.org.is_some()
            || self.exclude_user.is_some()
            || !self.topic.is_empty()
            || self.name.is_some()
            || self.license.is_some()
            || self.min_stars > 0
            || self.max_stars.is_some()
            || self.min_forks.is_some()
            || self.max_forks.is_some()
            || self.min_size.is_some()
            || self.max_size.is_some()
            || self.created_after.is_some()
            || self.created_before.is_some()
            || self.pushed_after.is_some()
            || self.no_forks
            || self.no_archived
            || self.no_empty
            || self.has_description
            || self.mirror
            || self.no_mirror
            || self.template
            || self.no_template
            || self.has_issues
            || self.no_issues
            || self.has_wiki
            || self.no_wiki
            || self.has_pages
            || self.no_pages
            || self.default_branch.is_some()
    }
}

/// Podzbiór pól zwracanych przez API GitHuba, których potrzebujemy.
#[derive(Debug, Clone, Deserialize)]
struct Repo {
    id: u64,
    #[serde(default)]
    name: String,
    full_name: String,
    html_url: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    stargazers_count: u64,
    #[serde(default)]
    forks_count: u64,
    /// Rozmiar repozytorium w kilobajtach; 0 oznacza repozytorium bez zawartości.
    #[serde(default)]
    size: u64,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    mirror_url: Option<String>,
    #[serde(default)]
    is_template: bool,
    #[serde(default)]
    has_issues: bool,
    #[serde(default)]
    has_wiki: bool,
    #[serde(default)]
    has_pages: bool,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    license: Option<License>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    pushed_at: Option<String>,
    #[serde(default)]
    owner: Option<Owner>,
}

#[derive(Debug, Clone, Deserialize)]
struct Owner {
    login: String,
}

#[derive(Debug, Clone, Deserialize)]
struct License {
    #[serde(default)]
    key: String,
    #[serde(default)]
    spdx_id: Option<String>,
    #[serde(default)]
    name: String,
}

/// Odpowiedź wyszukiwarki `/search/repositories`.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    total_count: u64,
    items: Vec<Repo>,
}

/// Kształt komunikatu błędu z API GitHuba.
#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default)]
    message: Option<String>,
}

/// Kształt wyniku dla `--json`.
#[derive(Debug, Serialize)]
struct RepoOut {
    full_name: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    topics: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<String>,
    stars: u64,
    forks: u64,
    size: u64,
    fork: bool,
    archived: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pushed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_branch: Option<String>,
}

impl From<&Repo> for RepoOut {
    fn from(repo: &Repo) -> Self {
        RepoOut {
            full_name: repo.full_name.clone(),
            url: repo.html_url.clone(),
            description: repo.description.clone(),
            language: repo.language.clone(),
            owner: repo.owner.as_ref().map(|o| o.login.clone()),
            topics: repo.topics.clone(),
            license: repo.license.as_ref().and_then(license_label),
            stars: repo.stargazers_count,
            forks: repo.forks_count,
            size: repo.size,
            fork: repo.fork,
            archived: repo.archived,
            created_at: repo.created_at.clone(),
            pushed_at: repo.pushed_at.clone(),
            default_branch: repo.default_branch.clone(),
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("repozier: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.count == 0 {
        bail!("--count musi być większe od zera");
    }

    if cli.max_id.is_some() && cli.has_filters() {
        eprintln!("repozier: --max-id działa tylko bez filtrów — ignoruję");
    }

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .into();

    let repos = pick_many(&cli, &agent, cli.token.as_deref())?;

    if cli.json {
        print_json(&repos)?;
    } else {
        for repo in &repos {
            println!("{}", repo.html_url);
        }
    }

    if cli.open {
        for repo in &repos {
            if let Err(err) = open_url(&repo.html_url) {
                eprintln!("repozier: nie udało się otworzyć {}: {err}", repo.html_url);
            }
        }
    }

    Ok(())
}

/// Drukuje wyniki jako JSON (obiekt przy jednym repo, tablica przy wielu).
fn print_json(repos: &[Repo]) -> Result<()> {
    let out: Vec<RepoOut> = repos.iter().map(RepoOut::from).collect();
    if out.len() == 1 {
        println!("{}", serde_json::to_string(&out[0])?);
    } else {
        println!("{}", serde_json::to_string(&out)?);
    }
    Ok(())
}

/// Losuje `count` różnych repozytoriów spełniających kryteria.
fn pick_many(cli: &Cli, agent: &ureq::Agent, token: Option<&str>) -> Result<Vec<Repo>> {
    if cli.has_filters() {
        pick_by_search(cli, agent, token)
    } else {
        pick_by_scan(cli, agent, token)
    }
}

/// Losowanie bez filtrów: skanuje publiczne ID, dzięki czemu każde
/// repozytorium ma w przybliżeniu równą szansę, niezależnie od popularności.
fn pick_by_scan(cli: &Cli, agent: &ureq::Agent, token: Option<&str>) -> Result<Vec<Repo>> {
    let mut chosen: Vec<Repo> = Vec::with_capacity(cli.count);
    let mut seen: HashSet<u64> = HashSet::new();
    let mut max_id = cli.max_id.unwrap_or(DEFAULT_MAX_ID);

    // Każde trafienie kosztuje jedno zapytanie; dajemy zapas na odrzucone wyniki.
    let budget = MAX_ATTEMPTS_PER_REPO * cli.count;
    for _ in 0..budget {
        if chosen.len() == cli.count {
            break;
        }

        let since = fastrand::u64(1..=max_id.max(1));
        let page = fetch_page(agent, token, since)?;

        if page.is_empty() {
            // Losowe `since` wypadło powyżej aktualnego maksimum ID.
            max_id = max_id.min(since.saturating_sub(1)).max(1);
            continue;
        }

        let mut candidates: Vec<&Repo> = page.iter().filter(|repo| !seen.contains(&repo.id)).collect();
        if candidates.is_empty() {
            continue;
        }
        fastrand::shuffle(&mut candidates);

        for repo in candidates {
            if chosen.len() == cli.count {
                break;
            }
            if seen.insert(repo.id) {
                // Skanowany listing nie zawiera wszystkich pól (m.in. gwiazdek,
                // rozmiaru), więc na potrzeby `--json` dociągamy pełne dane.
                let full = if cli.json {
                    fetch_repo(agent, token, &repo.full_name)?
                } else {
                    repo.clone()
                };
                chosen.push(full);
            }
        }
    }

    if chosen.len() < cli.count {
        bail!(
            "znaleziono {} z {} repozytoriów — spróbuj poluzować filtry",
            chosen.len(),
            cli.count
        );
    }

    Ok(chosen)
}

/// Losowanie z filtrami: korzysta z wyszukiwarki GitHuba (obsługuje wszystkie
/// filtry serwerowo) i wybiera losową stronę oraz losowy wynik z tej strony.
fn pick_by_search(cli: &Cli, agent: &ureq::Agent, token: Option<&str>) -> Result<Vec<Repo>> {
    let query = search_query(cli);
    let mut chosen: Vec<Repo> = Vec::with_capacity(cli.count);
    let mut seen: HashSet<u64> = HashSet::new();
    let mut tried: HashSet<u64> = HashSet::new();
    // Liczba stron poznana dopiero po pierwszej odpowiedzi (total_count).
    let mut pages: Option<u64> = None;

    for _ in 0..SEARCH_MAX_REQUESTS {
        if chosen.len() == cli.count {
            break;
        }

        // Pierwszą stronę losujemy na ślepo, kolejne już w znanym zakresie.
        let page = match pages {
            Some(pages) => match random_page(pages, &tried) {
                Some(page) => page,
                None => break,
            },
            None => fastrand::u64(1..=SEARCH_MAX_PAGES),
        };
        tried.insert(page);

        let response = fetch_search(agent, token, &query, page)?;
        let total = response.total_count.min(SEARCH_MAX_RESULTS);
        if total == 0 {
            bail!("brak repozytoriów spełniających podane filtry");
        }
        let known_pages = total.div_ceil(SEARCH_PAGE_SIZE as u64).max(1);
        pages = Some(known_pages);

        // Trafiliśmy poza zakres (np. mało wyników) — spróbuj ponownie.
        if page > known_pages {
            continue;
        }

        let mut matches: Vec<Repo> = response
            .items
            .into_iter()
            .filter(|repo| !seen.contains(&repo.id) && matches(repo, cli))
            .collect();
        fastrand::shuffle(&mut matches);

        for repo in matches {
            if chosen.len() == cli.count {
                break;
            }
            if seen.insert(repo.id) {
                chosen.push(repo);
            }
        }
    }

    if chosen.len() < cli.count {
        bail!(
            "znaleziono {} z {} repozytoriów — spróbuj poluzować filtry lub ustawić GITHUB_TOKEN",
            chosen.len(),
            cli.count
        );
    }

    Ok(chosen)
}

/// Losuje nieodwiedzoną jeszcze stronę wyników (1..=pages).
fn random_page(pages: u64, tried: &HashSet<u64>) -> Option<u64> {
    let candidates: Vec<u64> = (1..=pages).filter(|page| !tried.contains(page)).collect();
    if candidates.is_empty() {
        None
    } else {
        Some(candidates[fastrand::usize(..candidates.len())])
    }
}

/// Buduje zapytanie do wyszukiwarki GitHuba na podstawie filtrów.
fn search_query(cli: &Cli) -> String {
    let mut q = String::from("is:public");

    if let Some(v) = &cli.user {
        q.push_str(&format!(" user:{v}"));
    }
    if let Some(v) = &cli.org {
        q.push_str(&format!(" org:{v}"));
    }
    if let Some(v) = &cli.exclude_user {
        q.push_str(&format!(" -user:{v}"));
    }
    for v in &cli.topic {
        q.push_str(&format!(" topic:{v}"));
    }
    if let Some(v) = &cli.name {
        q.push_str(&format!(" in:name \"{}\"", v.replace('"', "")));
    }
    if let Some(v) = &cli.license {
        q.push_str(&format!(" license:{v}"));
    }
    if let Some(v) = &cli.language {
        q.push_str(&format!(" language:{v}"));
    }
    if cli.min_stars > 0 {
        q.push_str(&format!(" stars:>={}", cli.min_stars));
    }
    if let Some(v) = cli.max_stars {
        q.push_str(&format!(" stars:<={v}"));
    }
    if let Some(v) = cli.min_forks {
        q.push_str(&format!(" forks:>={v}"));
    }
    if let Some(v) = cli.max_forks {
        q.push_str(&format!(" forks:<={v}"));
    }
    if let Some(v) = cli.min_size {
        q.push_str(&format!(" size:>={v}"));
    }
    if let Some(v) = cli.max_size {
        q.push_str(&format!(" size:<={v}"));
    }
    if cli.no_empty {
        q.push_str(" size:>=1");
    }
    if let Some(v) = &cli.created_after {
        q.push_str(&format!(" created:>={v}"));
    }
    if let Some(v) = &cli.created_before {
        q.push_str(&format!(" created:<={v}"));
    }
    if let Some(v) = &cli.pushed_after {
        q.push_str(&format!(" pushed:>={v}"));
    }
    if cli.no_forks {
        q.push_str(" fork:false");
    }
    if cli.no_archived {
        q.push_str(" archived:false");
    }

    q
}

/// Wykonuje zapytanie GET i deserializuje odpowiedź JSON.
fn get_json<T: DeserializeOwned>(
    agent: &ureq::Agent,
    token: Option<&str>,
    url: &str,
    params: &[(&str, String)],
) -> Result<T> {
    let mut request = agent
        .get(url)
        .header("User-Agent", "repozier")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");

    for (key, value) in params {
        request = request.query(*key, value.as_str());
    }

    if let Some(token) = token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }

    let mut response = request
        .call()
        .map_err(|err| anyhow::anyhow!("błąd połączenia z GitHubem: {err}"))?;

    let status = response.status();
    let body = response.body_mut().read_to_string()?;

    if !status.is_success() {
        return Err(api_error(status.as_u16(), &body));
    }

    Ok(serde_json::from_str(&body)?)
}

/// Pobiera jedną stronę publicznych repozytoriów o ID większym niż `since`.
fn fetch_page(agent: &ureq::Agent, token: Option<&str>, since: u64) -> Result<Vec<Repo>> {
    get_json(
        agent,
        token,
        "https://api.github.com/repositories",
        &[
            ("since", since.to_string()),
            ("per_page", PAGE_SIZE.to_string()),
        ],
    )
}

/// Pobiera pełne dane pojedynczego repozytorium.
fn fetch_repo(agent: &ureq::Agent, token: Option<&str>, full_name: &str) -> Result<Repo> {
    let url = format!("https://api.github.com/repos/{full_name}");
    get_json(agent, token, &url, &[])
}

/// Pobiera jedną stronę wyników wyszukiwarki.
fn fetch_search(
    agent: &ureq::Agent,
    token: Option<&str>,
    query: &str,
    page: u64,
) -> Result<SearchResponse> {
    get_json(
        agent,
        token,
        "https://api.github.com/search/repositories",
        &[
            ("q", query.to_string()),
            ("page", page.to_string()),
            ("per_page", SEARCH_PAGE_SIZE.to_string()),
        ],
    )
}

/// Zamienia błąd HTTP GitHuba na czytelny komunikat (z treścią od GitHuba).
fn api_error(status: u16, body: &str) -> anyhow::Error {
    let hint = match status {
        401 => "nieprawidłowy lub wygasły token GitHub",
        403 | 429 => "limit API GitHuba wyczerpany — ustaw GITHUB_TOKEN lub spróbuj później",
        404 => "nie znaleziono zasobu",
        422 => "nieprawidłowe filtry (sprawdź daty i nazwy) lub zbyt głęboka paginacja",
        _ => "nieoczekiwana odpowiedź GitHuba",
    };

    match serde_json::from_str::<ApiError>(body)
        .ok()
        .and_then(|err| err.message)
        .filter(|msg| !msg.is_empty())
    {
        Some(message) => anyhow::anyhow!("GitHub zwrócił HTTP {status} ({hint}): {message}"),
        None => anyhow::anyhow!("GitHub zwrócił HTTP {status} ({hint})"),
    }
}

/// Sprawdza, czy repozytorium przechodzi wszystkie filtry z linii poleceń.
fn matches(repo: &Repo, cli: &Cli) -> bool {
    if cli.no_forks && repo.fork {
        return false;
    }
    if cli.no_archived && repo.archived {
        return false;
    }
    if cli.no_empty && repo.size == 0 {
        return false;
    }
    if cli.has_description && !has_description(repo) {
        return false;
    }
    if repo.stargazers_count < cli.min_stars {
        return false;
    }
    if let Some(max) = cli.max_stars
        && repo.stargazers_count > max
    {
        return false;
    }
    if let Some(min) = cli.min_forks
        && repo.forks_count < min
    {
        return false;
    }
    if let Some(max) = cli.max_forks
        && repo.forks_count > max
    {
        return false;
    }
    if let Some(min) = cli.min_size
        && repo.size < min
    {
        return false;
    }
    if let Some(max) = cli.max_size
        && repo.size > max
    {
        return false;
    }
    if let Some(wanted) = &cli.language
        && !repo
            .language
            .as_deref()
            .is_some_and(|lang| lang.eq_ignore_ascii_case(wanted))
    {
        return false;
    }
    if let Some(user) = &cli.user
        && !owner_is(repo, user)
    {
        return false;
    }
    if let Some(org) = &cli.org
        && !owner_is(repo, org)
    {
        return false;
    }
    if let Some(exclude) = &cli.exclude_user
        && owner_is(repo, exclude)
    {
        return false;
    }
    if cli
        .topic
        .iter()
        .any(|wanted| !repo.topics.iter().any(|t| t.eq_ignore_ascii_case(wanted)))
    {
        return false;
    }
    if let Some(needle) = &cli.name
        && !repo.name.to_lowercase().contains(&needle.to_lowercase())
    {
        return false;
    }
    if let Some(wanted) = &cli.license
        && !license_matches(repo, wanted)
    {
        return false;
    }
    if let Some(after) = &cli.created_after
        && !date_at_least(repo.created_at.as_deref(), after)
    {
        return false;
    }
    if let Some(before) = &cli.created_before
        && !date_before(repo.created_at.as_deref(), before)
    {
        return false;
    }
    if let Some(after) = &cli.pushed_after
        && !date_at_least(repo.pushed_at.as_deref(), after)
    {
        return false;
    }
    if cli.mirror && repo.mirror_url.is_none() {
        return false;
    }
    if cli.no_mirror && repo.mirror_url.is_some() {
        return false;
    }
    if cli.template && !repo.is_template {
        return false;
    }
    if cli.no_template && repo.is_template {
        return false;
    }
    if cli.has_issues && !repo.has_issues {
        return false;
    }
    if cli.no_issues && repo.has_issues {
        return false;
    }
    if cli.has_wiki && !repo.has_wiki {
        return false;
    }
    if cli.no_wiki && repo.has_wiki {
        return false;
    }
    if cli.has_pages && !repo.has_pages {
        return false;
    }
    if cli.no_pages && repo.has_pages {
        return false;
    }
    if let Some(branch) = &cli.default_branch
        && repo.default_branch.as_deref() != Some(branch.as_str())
    {
        return false;
    }
    true
}

/// Czy repozytorium ma niepusty opis?
fn has_description(repo: &Repo) -> bool {
    repo.description
        .as_deref()
        .is_some_and(|d| !d.trim().is_empty())
}

/// Czy data ISO 8601 jest równa lub późniejsza niż podana data (YYYY-MM-DD)?
/// Porównanie leksykalne działa, bo format ISO 8601 sortuje się leksykalnie.
fn date_at_least(actual: Option<&str>, wanted: &str) -> bool {
    actual.is_some_and(|date| date >= wanted)
}

/// Czy data ISO 8601 jest wcześniejsza niż podana data (YYYY-MM-DD)?
fn date_before(actual: Option<&str>, wanted: &str) -> bool {
    actual.is_some_and(|date| date < wanted)
}


/// Czy właściciel repozytorium to podany login (bez rozróżniania wielkości liter)?
fn owner_is(repo: &Repo, login: &str) -> bool {
    repo.owner
        .as_ref()
        .is_some_and(|owner| owner.login.eq_ignore_ascii_case(login))
}

/// Czy licencja repozytorium pasuje do podanej nazwy (klucz, SPDX lub nazwa)?
fn license_matches(repo: &Repo, wanted: &str) -> bool {
    match &repo.license {
        Some(license) => {
            license.key.eq_ignore_ascii_case(wanted)
                || license
                    .spdx_id
                    .as_deref()
                    .is_some_and(|spdx| spdx.eq_ignore_ascii_case(wanted))
                || license.name.eq_ignore_ascii_case(wanted)
        }
        None => false,
    }
}

/// Zwraca czytelną etykietę licencji (SPDX, a gdy brak — klucz).
fn license_label(license: &License) -> Option<String> {
    [license.spdx_id.as_deref(), Some(license.key.as_str())]
        .into_iter()
        .flatten()
        .find(|label| !label.is_empty())
        .map(str::to_owned)
}

/// Otwiera URL w domyślnej przeglądarce użytkownika.
fn open_url(url: &str) -> Result<()> {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).status()
    } else {
        Command::new("xdg-open").arg(url).status()
    }
    .map_err(|e| anyhow::anyhow!("nie można uruchomić przeglądarki: {e}"))?;

    if !status.success() {
        bail!("przeglądarka zwróciła kod {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Repo {
        serde_json::from_str(json).unwrap()
    }

    fn cli(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("repozier").chain(args.iter().copied()))
    }

    #[test]
    fn accepts_plain_public_repo() {
        let repo = parse(r#"{"id":1,"full_name":"a/b","html_url":"u","stargazers_count":10}"#);
        assert!(matches(&repo, &cli(&[])));
    }

    #[test]
    fn rejects_forks_when_asked() {
        let repo = parse(r#"{"id":2,"full_name":"a/b","html_url":"u","fork":true}"#);
        assert!(!matches(&repo, &cli(&["--no-forks"])));
        assert!(matches(&repo, &cli(&[])));
    }

    #[test]
    fn rejects_archived_when_asked() {
        let repo = parse(r#"{"id":3,"full_name":"a/b","html_url":"u","archived":true}"#);
        assert!(!matches(&repo, &cli(&["--no-archived"])));
    }

    #[test]
    fn rejects_empty_repos_when_asked() {
        let empty = parse(r#"{"id":9,"full_name":"a/b","html_url":"u","size":0}"#);
        assert!(!matches(&empty, &cli(&["--no-empty"])));
        assert!(matches(&empty, &cli(&[])));

        let filled = parse(r#"{"id":10,"full_name":"a/b","html_url":"u","size":42}"#);
        assert!(matches(&filled, &cli(&["--no-empty"])));
    }

    #[test]
    fn respects_min_stars() {
        let repo = parse(r#"{"id":4,"full_name":"a/b","html_url":"u","stargazers_count":3}"#);
        assert!(!matches(&repo, &cli(&["--min-stars", "100"])));
        assert!(matches(&repo, &cli(&["--min-stars", "1"])));
    }

    #[test]
    fn respects_max_stars() {
        let repo = parse(r#"{"id":4,"full_name":"a/b","html_url":"u","stargazers_count":500}"#);
        assert!(!matches(&repo, &cli(&["--max-stars", "100"])));
        assert!(matches(&repo, &cli(&["--max-stars", "1000"])));
    }

    #[test]
    fn respects_fork_range() {
        let repo = parse(r#"{"id":4,"full_name":"a/b","html_url":"u","forks_count":7}"#);
        assert!(!matches(&repo, &cli(&["--min-forks", "10"])));
        assert!(matches(&repo, &cli(&["--min-forks", "5"])));
        assert!(!matches(&repo, &cli(&["--max-forks", "5"])));
        assert!(matches(&repo, &cli(&["--max-forks", "10"])));
    }

    #[test]
    fn respects_size_range() {
        let repo = parse(r#"{"id":4,"full_name":"a/b","html_url":"u","size":100}"#);
        assert!(!matches(&repo, &cli(&["--min-size", "200"])));
        assert!(matches(&repo, &cli(&["--min-size", "50"])));
        assert!(!matches(&repo, &cli(&["--max-size", "50"])));
        assert!(matches(&repo, &cli(&["--max-size", "200"])));
    }

    #[test]
    fn filters_language_case_insensitively() {
        let repo = parse(r#"{"id":5,"full_name":"a/b","html_url":"u","language":"Python"}"#);
        assert!(matches(&repo, &cli(&["--language", "python"])));
        assert!(!matches(&repo, &cli(&["--language", "rust"])));

        let no_language = parse(r#"{"id":6,"full_name":"a/b","html_url":"u"}"#);
        assert!(!matches(&no_language, &cli(&["--language", "rust"])));
    }

    #[test]
    fn filters_by_user_and_org() {
        let repo = parse(
            r#"{"id":11,"full_name":"octocat/hello","html_url":"u","owner":{"login":"octocat"}}"#,
        );
        assert!(matches(&repo, &cli(&["--user", "octocat"])));
        assert!(matches(&repo, &cli(&["--user", "OCTOCAT"])));
        assert!(!matches(&repo, &cli(&["--user", "someone"])));
        assert!(matches(&repo, &cli(&["--org", "octocat"])));
        assert!(!matches(&repo, &cli(&["--exclude-user", "octocat"])));
        assert!(matches(&repo, &cli(&["--exclude-user", "someone"])));
    }

    #[test]
    fn filters_by_topic() {
        let repo = parse(r#"{"id":12,"full_name":"a/b","html_url":"u","topics":["cli","rust"]}"#);
        assert!(matches(&repo, &cli(&["--topic", "CLI"])));
        assert!(matches(&repo, &cli(&["--topic", "cli", "--topic", "rust"])));
        assert!(!matches(&repo, &cli(&["--topic", "python"])));
    }

    #[test]
    fn filters_by_name() {
        let repo = parse(r#"{"id":13,"full_name":"a/b","name":"Awesome-CLI","html_url":"u"}"#);
        assert!(matches(&repo, &cli(&["--name", "awesome"])));
        assert!(!matches(&repo, &cli(&["--name", "gui"])));
    }

    #[test]
    fn filters_by_license() {
        let repo = parse(
            r#"{"id":14,"full_name":"a/b","html_url":"u","license":{"key":"mit","spdx_id":"MIT","name":"MIT License"}}"#,
        );
        assert!(matches(&repo, &cli(&["--license", "mit"])));
        assert!(matches(&repo, &cli(&["--license", "MIT"])));
        assert!(!matches(&repo, &cli(&["--license", "apache-2.0"])));

        let no_license = parse(r#"{"id":15,"full_name":"a/b","html_url":"u"}"#);
        assert!(!matches(&no_license, &cli(&["--license", "mit"])));
    }

    #[test]
    fn filters_by_dates() {
        let repo = parse(
            r#"{"id":16,"full_name":"a/b","html_url":"u","created_at":"2020-06-15T10:00:00Z","pushed_at":"2024-01-02T00:00:00Z"}"#,
        );
        assert!(matches(&repo, &cli(&["--created-after", "2020-01-01"])));
        assert!(!matches(&repo, &cli(&["--created-after", "2021-01-01"])));
        assert!(matches(&repo, &cli(&["--created-before", "2021-01-01"])));
        assert!(!matches(&repo, &cli(&["--created-before", "2020-01-01"])));
        assert!(matches(&repo, &cli(&["--pushed-after", "2023-01-01"])));
        assert!(!matches(&repo, &cli(&["--pushed-after", "2025-01-01"])));
    }

    #[test]
    fn filters_by_flags() {
        let repo = parse(
            r#"{"id":17,"full_name":"a/b","html_url":"u","mirror_url":"git://x","is_template":true,"has_issues":true,"has_wiki":false,"has_pages":true,"default_branch":"main"}"#,
        );
        assert!(matches(&repo, &cli(&["--mirror"])));
        assert!(!matches(&repo, &cli(&["--no-mirror"])));
        assert!(matches(&repo, &cli(&["--template"])));
        assert!(!matches(&repo, &cli(&["--no-template"])));
        assert!(matches(&repo, &cli(&["--has-issues"])));
        assert!(!matches(&repo, &cli(&["--no-issues"])));
        assert!(matches(&repo, &cli(&["--no-wiki"])));
        assert!(!matches(&repo, &cli(&["--has-wiki"])));
        assert!(matches(&repo, &cli(&["--has-pages"])));
        assert!(matches(&repo, &cli(&["--default-branch", "main"])));
        assert!(!matches(&repo, &cli(&["--default-branch", "master"])));
    }

    #[test]
    fn requires_non_empty_description() {
        let blank = parse(r#"{"id":7,"full_name":"a/b","html_url":"u","description":"   "}"#);
        assert!(!matches(&blank, &cli(&["--has-description"])));

        let filled = parse(r#"{"id":8,"full_name":"a/b","html_url":"u","description":"ok"}"#);
        assert!(matches(&filled, &cli(&["--has-description"])));
    }

    #[test]
    fn builds_search_query_from_filters() {
        let query = search_query(&cli(&[
            "--user",
            "octocat",
            "--topic",
            "cli",
            "--language",
            "rust",
            "--min-stars",
            "10",
            "--no-forks",
        ]));
        assert!(query.contains("user:octocat"));
        assert!(query.contains("topic:cli"));
        assert!(query.contains("language:rust"));
        assert!(query.contains("stars:>=10"));
        assert!(query.contains("fork:false"));
    }

    #[test]
    fn detects_when_filters_are_present() {
        assert!(!cli(&[]).has_filters());
        assert!(cli(&["--no-empty"]).has_filters());
        assert!(cli(&["--user", "octocat"]).has_filters());
    }
}
