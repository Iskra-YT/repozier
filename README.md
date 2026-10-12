# repozier

A random GitHub repository straight from the terminal.

Run `repozier` and you get a link to a random public repository.

```console
$ repozier
https://github.com/rust-lang/mdBook
```

## Installation

Build from source (requires [Rust](https://rustup.rs)):

```console
$ cargo build --release
$ ./target/release/repozier
```

or install it locally so `repozier` is available in your `PATH`:

```console
$ cargo install --path .
```

## Usage

```
repozier — a random GitHub repository

USAGE:
    repozier [OPTIONS]

FILTERS:
    -l, --language <LANGUAGE>     Filter by language (e.g. rust, python, go)
    -u, --user <USER>             Only repositories of a user or organization
    -o, --org <ORG>               Only repositories of an organization
        --exclude-user <USER>     Exclude repositories of a user
    -t, --topic <TOPIC>           Only with the given topic (may be repeated)
        --name <TEXT>             Only when the name contains the text
        --license <LICENSE>       Only under the given license (e.g. mit, apache-2.0)
        --min-stars <N>           Minimum number of stars [default: 0]
        --max-stars <N>           Maximum number of stars
        --min-forks <N>           Minimum number of forks
        --max-forks <N>           Maximum number of forks
        --min-size <KB>           Minimum size in kilobytes
        --max-size <KB>           Maximum size in kilobytes
        --created-after <DATE>    Created after the date (YYYY-MM-DD)
        --created-before <DATE>   Created before the date (YYYY-MM-DD)
        --pushed-after <DATE>     Last pushed after the date (YYYY-MM-DD)
        --no-forks                Skip forks
        --no-archived             Skip archived repositories
        --no-empty                Skip empty repositories (no content)
        --has-description         Only repositories with a description
        --mirror / --no-mirror    Only mirrors / skip mirrors
        --template / --no-template  Only templates / skip templates
        --has-issues / --no-issues  With Issues enabled / disabled
        --has-wiki / --no-wiki    With the wiki enabled / disabled
        --has-pages / --no-pages  With GitHub Pages enabled / disabled
        --default-branch <NAME>   Only with the given default branch

OUTPUT:
    -n, --count <N>           How many repositories to print [default: 1]
        --json                Print the result as JSON
        --open                Open the link in the browser
        --max-id <ID>         Upper bound of the random ID (without filters)
        --token <TOKEN>       GitHub token (from GITHUB_TOKEN by default)
    -h, --help                Help
    -V, --version             Version
```

A **random** repository matching the given filters is always selected —
not "the first on the list" or "the most popular".

### Examples

A random Rust repo with at least 100 stars:

```console
$ repozier --language rust --min-stars 100
```

A random repo from a specific user or organization:

```console
$ repozier --user rust-lang
$ repozier --org tokio-rs
```

Three random Python repositories, without forks and without empty ones:

```console
$ repozier -n 3 --language python --no-forks --no-empty
```

New, active projects under the MIT license:

```console
$ repozier --license mit --created-after 2024-01-01 --pushed-after 2025-01-01
```

Repositories with a given topic:

```console
$ repozier --topic cli --language go --min-stars 500
```

Output as JSON (e.g. for further processing):

```console
$ repozier --json
{"full_name":"owner/name","url":"https://github.com/owner/name", ...}
```

## GitHub token

Without a token, the public GitHub API is used with a limit of ~60 requests
per hour (search: ~10 requests per minute). For a higher limit, set a token
(it does not need any scopes, public read access is enough):

```console
$ export GITHUB_TOKEN=ghp_xxx
$ repozier
```

You can also pass the token with the `--token` flag, but the environment
variable is safer (it does not end up in your shell history).

## How it picks a repository

`repozier` works in two modes:

- **Without filters** it uses the `/repositories?since=<id>` endpoint, which
  returns public repositories sorted by ID. It picks a random ID, fetches a
  page of results and chooses a random repo. This makes the distribution
  practically uniform and independent of popularity.
- **With filters** it uses the `/search/repositories` search API, which handles
  all filters server-side. The program fetches a random page of results and
  picks a random repository from that page. The search API only exposes the
  first 1000 results, so very broad queries sample that range.

In both modes the choice is always random.

## License

MIT — see [LICENSE](LICENSE).
