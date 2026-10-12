//! repozier — wypisuje link do losowego repozytorium z GitHuba.

use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

/// Górna granica ID repozytoriów. ID rosną z czasem, więc od czasu do czasu
/// warto tę wartość podnieść. Jeśli losowe `since` wypadnie powyżej
/// aktualnego maksimum, program sam zmniejszy zakres i spróbuje ponownie.
const DEFAULT_MAX_ID: u64 = 1_400_000_000;
/// Ile repozytoriów GitHub zwraca na jedną stronę (maksymalnie 100).
const PAGE_SIZE: u32 = 100;
/// Ile razy próbować na jedno repozytorium, zanim uznamy filtry za zbyt wąskie.
const MAX_ATTEMPTS_PER_REPO: usize = 20;

#[derive(Parser, Debug)]
#[command(
    name = "repozier",
    version,
    about = "Losowe repozytorium z GitHuba prosto z terminala",
    after_help = "Bez tokenu obowiazuje limit ~60 zapytan/h. Ustaw GITHUB_TOKEN, aby go podniesc."
)]
struct Cli {
    /// Ile repozytoriów wypisać
    #[arg(short = 'n', long, default_value_t = 1)]
    count: usize,

    /// Filtruj po języku programowania (np. rust, python, go)
    #[arg(short = 'l', long, value_name = "JEZYK")]
    language: Option<String>,

    /// Minimalna liczba gwiazdek
    #[arg(long, default_value_t = 0, value_name = "N")]
    min_stars: u64,

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

    /// Wypisz wynik jako JSON
    #[arg(long)]
    json: bool,

    /// Otwórz link w przeglądarce
    #[arg(long)]
    open: bool,

    /// Górna granica losowanego ID (zaawansowane)
    #[arg(long, value_name = "ID")]
    max_id: Option<u64>,

    /// Token GitHub (domyślnie brany ze zmiennej GITHUB_TOKEN)
    #[arg(long, env = "GITHUB_TOKEN", hide_env_values = true, value_name = "TOKEN")]
    token: Option<String>,
}

/// Podzbiór pól zwracanych przez API GitHuba, których potrzebujemy.
#[derive(Debug, Clone, Deserialize)]
struct Repo {
    id: u64,
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
    stars: u64,
    forks: u64,
    size: u64,
    fork: bool,
    archived: bool,
}

impl From<&Repo> for RepoOut {
    fn from(repo: &Repo) -> Self {
        RepoOut {
            full_name: repo.full_name.clone(),
            url: repo.html_url.clone(),
            description: repo.description.clone(),
            language: repo.language.clone(),
            stars: repo.stargazers_count,
            forks: repo.forks_count,
            size: repo.size,
            fork: repo.fork,
            archived: repo.archived,
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

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
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

        let candidates: Vec<&Repo> = page.iter().filter(|repo| matches(repo, cli)).collect();
        if candidates.is_empty() {
            continue;
        }

        let repo = candidates[fastrand::usize(..candidates.len())];
        if seen.insert(repo.id) {
            chosen.push(repo.clone());
        }
    }

    if chosen.len() < cli.count {
        bail!(
            "znaleziono {} z {} repozytoriów — spróbuj poluzować filtry lub zwiększyć --min-stars",
            chosen.len(),
            cli.count
        );
    }

    Ok(chosen)
}

/// Pobiera jedną stronę publicznych repozytoriów o ID większym niż `since`.
fn fetch_page(agent: &ureq::Agent, token: Option<&str>, since: u64) -> Result<Vec<Repo>> {
    let url = format!("https://api.github.com/repositories?since={since}&per_page={PAGE_SIZE}");

    let mut request = agent
        .get(&url)
        .header("User-Agent", "repozier")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");

    if let Some(token) = token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }

    let mut response = request.call().map_err(explain_api_error)?;
    let body = response.body_mut().read_to_string()?;
    let repos: Vec<Repo> = serde_json::from_str(&body)?;
    Ok(repos)
}

/// Zamienia błędy API na czytelne komunikaty.
fn explain_api_error(err: ureq::Error) -> anyhow::Error {
    match err {
        ureq::Error::StatusCode(code) => {
            let hint = match code {
                401 => "nieprawidłowy lub wygasły token GitHub",
                403 => "limit API GitHuba wyczerpany — ustaw GITHUB_TOKEN",
                404 => "nie znaleziono zasobu",
                429 => "za dużo zapytań — spróbuj za chwilę",
                _ => "nieoczekiwana odpowiedź GitHuba",
            };
            anyhow::anyhow!("GitHub zwrócił HTTP {code} ({hint})")
        }
        other => anyhow::anyhow!("błąd połączenia z GitHubem: {other}"),
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
    if cli.has_description {
        let described = repo.description.as_deref().is_some_and(|d| !d.trim().is_empty());
        if !described {
            return false;
        }
    }
    if repo.stargazers_count < cli.min_stars {
        return false;
    }
    if let Some(wanted) = &cli.language {
        match &repo.language {
            Some(lang) if lang.eq_ignore_ascii_case(wanted) => {}
            _ => return false,
        }
    }
    true
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
    fn filters_language_case_insensitively() {
        let repo = parse(r#"{"id":5,"full_name":"a/b","html_url":"u","language":"Python"}"#);
        assert!(matches(&repo, &cli(&["--language", "python"])));
        assert!(!matches(&repo, &cli(&["--language", "rust"])));

        let no_language = parse(r#"{"id":6,"full_name":"a/b","html_url":"u"}"#);
        assert!(!matches(&no_language, &cli(&["--language", "rust"])));
    }

    #[test]
    fn requires_non_empty_description() {
        let blank = parse(r#"{"id":7,"full_name":"a/b","html_url":"u","description":"   "}"#);
        assert!(!matches(&blank, &cli(&["--has-description"])));

        let filled = parse(r#"{"id":8,"full_name":"a/b","html_url":"u","description":"ok"}"#);
        assert!(matches(&filled, &cli(&["--has-description"])));
    }
}
