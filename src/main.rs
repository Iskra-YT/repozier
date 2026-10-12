//! repozier — prints a link to a random GitHub repository.

use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Upper bound for repository IDs. IDs grow over time, so it is worth raising
/// this value from time to time. If a random `since` lands above the current
/// maximum, the program shrinks the range itself and retries.
const DEFAULT_MAX_ID: u64 = 1_400_000_000;
/// How many repositories GitHub returns per page (100 max).
const PAGE_SIZE: u32 = 100;
/// Page size in the GitHub search API (100 max).
const SEARCH_PAGE_SIZE: u32 = 100;
/// How many attempts per repository before we consider the filters too narrow.
const MAX_ATTEMPTS_PER_REPO: usize = 20;
/// The GitHub search API only exposes the first 1000 results.
const SEARCH_MAX_RESULTS: u64 = 1000;
/// Maximum page number in the search API with `SEARCH_PAGE_SIZE`.
const SEARCH_MAX_PAGES: u64 = SEARCH_MAX_RESULTS / SEARCH_PAGE_SIZE as u64;
/// Maximum number of search API requests per run.
const SEARCH_MAX_REQUESTS: usize = 8;

#[derive(Parser, Debug)]
#[command(
    name = "repozier",
    version,
    about = "A random GitHub repository straight from the terminal",
    after_help = "Without a token the limit is ~60 requests/h (search: ~10/min).\nSet GITHUB_TOKEN to raise the limits.\n\nA random repository matching the given filters is always selected."
)]
struct Cli {
    /// How many repositories to print
    #[arg(short = 'n', long, default_value_t = 1)]
    count: usize,

    /// Filter by programming language (e.g. rust, python, go)
    #[arg(short = 'l', long, value_name = "LANGUAGE")]
    language: Option<String>,

    /// Only repositories of the given user or organization
    #[arg(short = 'u', long, value_name = "USER")]
    user: Option<String>,

    /// Only repositories of the given organization
    #[arg(short = 'o', long, value_name = "ORG")]
    org: Option<String>,

    /// Exclude repositories of the given user (may be given multiple times)
    #[arg(long, value_name = "USER")]
    exclude_user: Option<String>,

    /// Only repositories with the given topic (may be given multiple times)
    #[arg(short = 't', long = "topic", value_name = "TOPIC")]
    topic: Vec<String>,

    /// Only repositories whose name contains the given text
    #[arg(long, value_name = "TEXT")]
    name: Option<String>,

    /// Only repositories under the given license (e.g. mit, apache-2.0)
    #[arg(long, value_name = "LICENSE")]
    license: Option<String>,

    /// Minimum number of stars
    #[arg(long, default_value_t = 0, value_name = "N")]
    min_stars: u64,

    /// Maximum number of stars
    #[arg(long, value_name = "N")]
    max_stars: Option<u64>,

    /// Minimum number of forks
    #[arg(long, value_name = "N")]
    min_forks: Option<u64>,

    /// Maximum number of forks
    #[arg(long, value_name = "N")]
    max_forks: Option<u64>,

    /// Minimum repository size in kilobytes
    #[arg(long, value_name = "KB")]
    min_size: Option<u64>,

    /// Maximum repository size in kilobytes
    #[arg(long, value_name = "KB")]
    max_size: Option<u64>,

    /// Only repositories created after the date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    created_after: Option<String>,

    /// Only repositories created before the date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    created_before: Option<String>,

    /// Only repositories last pushed after the date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pushed_after: Option<String>,

    /// Skip forks
    #[arg(long)]
    no_forks: bool,

    /// Skip archived repositories
    #[arg(long)]
    no_archived: bool,

    /// Skip empty repositories (no content)
    #[arg(long)]
    no_empty: bool,

    /// Only repositories with a description
    #[arg(long)]
    has_description: bool,

    /// Only mirror repositories
    #[arg(long)]
    mirror: bool,

    /// Skip mirrors
    #[arg(long)]
    no_mirror: bool,

    /// Only template repositories
    #[arg(long)]
    template: bool,

    /// Skip template repositories
    #[arg(long)]
    no_template: bool,

    /// Only repositories with Issues enabled
    #[arg(long)]
    has_issues: bool,

    /// Only repositories with Issues disabled
    #[arg(long)]
    no_issues: bool,

    /// Only repositories with the wiki enabled
    #[arg(long)]
    has_wiki: bool,

    /// Only repositories with the wiki disabled
    #[arg(long)]
    no_wiki: bool,

    /// Only repositories with GitHub Pages enabled
    #[arg(long)]
    has_pages: bool,

    /// Only repositories with GitHub Pages disabled
    #[arg(long)]
    no_pages: bool,

    /// Only repositories with the given default branch
    #[arg(long, value_name = "NAME")]
    default_branch: Option<String>,

    /// Print the result as JSON
    #[arg(long)]
    json: bool,

    /// Open the link in the browser
    #[arg(long)]
    open: bool,

    /// Upper bound of the random ID (advanced, only without filters)
    #[arg(long, value_name = "ID")]
    max_id: Option<u64>,

    /// GitHub token (taken from the GITHUB_TOKEN variable by default)
    #[arg(long, env = "GITHUB_TOKEN", hide_env_values = true, value_name = "TOKEN")]
    token: Option<String>,
}

impl Cli {
    /// Was any content filter given? If so, we pick via the GitHub search API
    /// (it handles filters server-side). Otherwise we scan IDs, which yields a
    /// uniform distribution across all of GitHub.
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

/// A subset of the fields returned by the GitHub API that we need.
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
    /// Repository size in kilobytes; 0 means a repository with no content.
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

/// Response of the `/search/repositories` search API.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    total_count: u64,
    items: Vec<Repo>,
}

/// Shape of an error message from the GitHub API.
#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default)]
    message: Option<String>,
}

/// Output shape for `--json`.
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
        bail!("--count must be greater than zero");
    }

    if cli.max_id.is_some() && cli.has_filters() {
        eprintln!("repozier: --max-id only works without filters — ignoring it");
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
                eprintln!("repozier: failed to open {}: {err}", repo.html_url);
            }
        }
    }

    Ok(())
}

/// Prints the results as JSON (an object for one repo, an array for many).
fn print_json(repos: &[Repo]) -> Result<()> {
    let out: Vec<RepoOut> = repos.iter().map(RepoOut::from).collect();
    if out.len() == 1 {
        println!("{}", serde_json::to_string(&out[0])?);
    } else {
        println!("{}", serde_json::to_string(&out)?);
    }
    Ok(())
}

/// Picks `count` distinct repositories matching the criteria.
fn pick_many(cli: &Cli, agent: &ureq::Agent, token: Option<&str>) -> Result<Vec<Repo>> {
    if cli.has_filters() {
        pick_by_search(cli, agent, token)
    } else {
        pick_by_scan(cli, agent, token)
    }
}

/// Picking without filters: scans public IDs, so every repository has roughly
/// the same chance regardless of popularity.
fn pick_by_scan(cli: &Cli, agent: &ureq::Agent, token: Option<&str>) -> Result<Vec<Repo>> {
    let mut chosen: Vec<Repo> = Vec::with_capacity(cli.count);
    let mut seen: HashSet<u64> = HashSet::new();
    let mut max_id = cli.max_id.unwrap_or(DEFAULT_MAX_ID);

    // Every attempt costs one request; we add slack for rejected results.
    let budget = MAX_ATTEMPTS_PER_REPO * cli.count;
    for _ in 0..budget {
        if chosen.len() == cli.count {
            break;
        }

        let since = fastrand::u64(1..=max_id.max(1));
        let page = fetch_page(agent, token, since)?;

        if page.is_empty() {
            // The random `since` landed above the current maximum ID.
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
                // The scanned listing does not include all fields (e.g. stars,
                // size), so for `--json` we fetch the full data.
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
            "found {} of {} repositories — try relaxing the filters",
            chosen.len(),
            cli.count
        );
    }

    Ok(chosen)
}

/// Picking with filters: uses the GitHub search API (it handles all filters
/// server-side) and picks a random page and a random result from that page.
fn pick_by_search(cli: &Cli, agent: &ureq::Agent, token: Option<&str>) -> Result<Vec<Repo>> {
    let query = search_query(cli);
    let mut chosen: Vec<Repo> = Vec::with_capacity(cli.count);
    let mut seen: HashSet<u64> = HashSet::new();
    let mut tried: HashSet<u64> = HashSet::new();
    // The number of pages is only known after the first response (total_count).
    let mut pages: Option<u64> = None;

    for _ in 0..SEARCH_MAX_REQUESTS {
        if chosen.len() == cli.count {
            break;
        }

        // We pick the first page blindly, the next ones within the known range.
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
            bail!("no repositories match the given filters");
        }
        let known_pages = total.div_ceil(SEARCH_PAGE_SIZE as u64).max(1);
        pages = Some(known_pages);

        // We landed out of range (e.g. few results) — try again.
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
            "found {} of {} repositories — try relaxing the filters or setting GITHUB_TOKEN",
            chosen.len(),
            cli.count
        );
    }

    Ok(chosen)
}

/// Picks a not-yet-visited page of results (1..=pages).
fn random_page(pages: u64, tried: &HashSet<u64>) -> Option<u64> {
    let candidates: Vec<u64> = (1..=pages).filter(|page| !tried.contains(page)).collect();
    if candidates.is_empty() {
        None
    } else {
        Some(candidates[fastrand::usize(..candidates.len())])
    }
}

/// Builds a GitHub search query from the filters.
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

/// Performs a GET request and deserializes the JSON response.
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
        .map_err(|err| anyhow::anyhow!("connection to GitHub failed: {err}"))?;

    let status = response.status();
    let body = response.body_mut().read_to_string()?;

    if !status.is_success() {
        return Err(api_error(status.as_u16(), &body));
    }

    Ok(serde_json::from_str(&body)?)
}

/// Fetches one page of public repositories with an ID greater than `since`.
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

/// Fetches the full data of a single repository.
fn fetch_repo(agent: &ureq::Agent, token: Option<&str>, full_name: &str) -> Result<Repo> {
    let url = format!("https://api.github.com/repos/{full_name}");
    get_json(agent, token, &url, &[])
}

/// Fetches one page of search results.
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

/// Turns a GitHub HTTP error into a readable message (including GitHub's own text).
fn api_error(status: u16, body: &str) -> anyhow::Error {
    let hint = match status {
        401 => "invalid or expired GitHub token",
        403 | 429 => "GitHub API limit exhausted — set GITHUB_TOKEN or try again later",
        404 => "resource not found",
        422 => "invalid filters (check dates and names) or pagination too deep",
        _ => "unexpected response from GitHub",
    };

    match serde_json::from_str::<ApiError>(body)
        .ok()
        .and_then(|err| err.message)
        .filter(|msg| !msg.is_empty())
    {
        Some(message) => anyhow::anyhow!("GitHub returned HTTP {status} ({hint}): {message}"),
        None => anyhow::anyhow!("GitHub returned HTTP {status} ({hint})"),
    }
}

/// Checks whether a repository passes all command-line filters.
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

/// Does the repository have a non-empty description?
fn has_description(repo: &Repo) -> bool {
    repo.description
        .as_deref()
        .is_some_and(|d| !d.trim().is_empty())
}

/// Is the ISO 8601 date equal to or later than the given date (YYYY-MM-DD)?
/// Lexicographic comparison works because ISO 8601 sorts lexicographically.
fn date_at_least(actual: Option<&str>, wanted: &str) -> bool {
    actual.is_some_and(|date| date >= wanted)
}

/// Is the ISO 8601 date earlier than the given date (YYYY-MM-DD)?
fn date_before(actual: Option<&str>, wanted: &str) -> bool {
    actual.is_some_and(|date| date < wanted)
}


/// Is the repository owner the given login (case-insensitive)?
fn owner_is(repo: &Repo, login: &str) -> bool {
    repo.owner
        .as_ref()
        .is_some_and(|owner| owner.login.eq_ignore_ascii_case(login))
}

/// Does the repository license match the given name (key, SPDX or name)?
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

/// Returns a readable license label (SPDX, or the key when missing).
fn license_label(license: &License) -> Option<String> {
    [license.spdx_id.as_deref(), Some(license.key.as_str())]
        .into_iter()
        .flatten()
        .find(|label| !label.is_empty())
        .map(str::to_owned)
}

/// Opens a URL in the user's default browser.
fn open_url(url: &str) -> Result<()> {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).status()
    } else {
        Command::new("xdg-open").arg(url).status()
    }
    .map_err(|e| anyhow::anyhow!("could not launch the browser: {e}"))?;

    if !status.success() {
        bail!("browser returned code {}", status.code().unwrap_or(-1));
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
