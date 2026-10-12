# repozier

Losowe repozytorium z GitHuba prosto z terminala.

Odpalasz `repozier` i dostajesz link do losowego publicznego repozytorium.

```console
$ repozier
https://github.com/rust-lang/mdBook
```

## Instalacja

Zbuduj z źródła (wymaga [Rusta](https://rustup.rs)):

```console
$ cargo build --release
$ ./target/release/repozier
```

albo zainstaluj lokalnie, żeby `repozier` był dostępny w `PATH`:

```console
$ cargo install --path .
```

## Użycie

```
repozier — losowe repozytorium z GitHuba

USAGE:
    repozier [OPTIONS]

FILTRY:
    -l, --language <JEZYK>        Filtruj po języku (np. rust, python, go)
    -u, --user <UZYTKOWNIK>       Tylko repozytoria użytkownika lub organizacji
    -o, --org <ORGANIZACJA>       Tylko repozytoria organizacji
        --exclude-user <USER>     Wyklucz repozytoria użytkownika
    -t, --topic <TEMAT>           Tylko z danym tematem (można podać wielokrotnie)
        --name <TEKST>            Tylko gdy nazwa zawiera tekst
        --license <LICENCJA>      Tylko na danej licencji (np. mit, apache-2.0)
        --min-stars <N>           Minimalna liczba gwiazdek [default: 0]
        --max-stars <N>           Maksymalna liczba gwiazdek
        --min-forks <N>           Minimalna liczba forków
        --max-forks <N>           Maksymalna liczba forków
        --min-size <KB>           Minimalny rozmiar w kilobajtach
        --max-size <KB>           Maksymalny rozmiar w kilobajtach
        --created-after <DATA>    Utworzone po dacie (YYYY-MM-DD)
        --created-before <DATA>   Utworzone przed datą (YYYY-MM-DD)
        --pushed-after <DATA>     Ostatni push po dacie (YYYY-MM-DD)
        --no-forks                Pomijaj forki
        --no-archived             Pomijaj zarchiwizowane
        --no-empty                Pomijaj puste repozytoria (bez zawartości)
        --has-description         Tylko repozytoria z opisem
        --mirror / --no-mirror    Tylko mirrory / bez mirrorów
        --template / --no-template  Tylko szablony / bez szablonów
        --has-issues / --no-issues  Z włączonymi / wyłączonymi Issues
        --has-wiki / --no-wiki    Z włączonym / wyłączonym wiki
        --has-pages / --no-pages  Z włączonym / wyłączonym GitHub Pages
        --default-branch <NAZWA>  Tylko z daną domyślną gałęzią

WYJŚCIE:
    -n, --count <N>           Ile repozytoriów wypisać [default: 1]
        --json                Wypisz wynik jako JSON
        --open                Otwórz link w przeglądarce
        --max-id <ID>         Górna granica losowanego ID (bez filtrów)
        --token <TOKEN>       Token GitHub (domyślnie z GITHUB_TOKEN)
    -h, --help                Pomoc
    -V, --version             Wersja
```

Zawsze wybierane jest **losowe** repozytorium spełniające podane filtry —
nie „pierwsze z listy” ani „najpopularniejsze”.

### Przykłady

Losowe repo w Rust z co najmniej 100 gwiazdkami:

```console
$ repozier --language rust --min-stars 100
```

Losowe repo od konkretnego użytkownika lub organizacji:

```console
$ repozier --user rust-lang
$ repozier --org tokio-rs
```

Trzy losowe repozytoria w Pythonie, bez forków i bez pustych:

```console
$ repozier -n 3 --language python --no-forks --no-empty
```

Nowe, aktywne projekty na licencji MIT:

```console
$ repozier --license mit --created-after 2024-01-01 --pushed-after 2025-01-01
```

Repozytoria z danym tematem:

```console
$ repozier --topic cli --language go --min-stars 500
```

Wynik jako JSON (np. do dalszego przetwarzania):

```console
$ repozier --json
{"full_name":"owner/name","url":"https://github.com/owner/name", ...}
```

## Token GitHub

Bez tokenu działa publiczne API GitHuba z limitem ok. 60 zapytań na godzinę
(wyszukiwarka: ok. 10 zapytań na minutę). Dla wyższego limitu ustaw token
(nie musi mieć żadnych uprawnień, wystarczy publiczny dostęp do odczytu):

```console
$ export GITHUB_TOKEN=ghp_xxx
$ repozier
```

Token można też podać flagą `--token`, ale zmienna środowiskowa jest
bezpieczniejsza (nie trafia do historii powłoki).

## Jak losuje

`repozier` działa w dwóch trybach:

- **Bez filtrów** korzysta z endpointu `/repositories?since=<id>`, który zwraca
  publiczne repozytoria posortowane po ID. Losuje ID, pobiera stronę wyników
  i wybiera losowe repo. Dzięki temu rozkład jest w praktyce równomierny
  i nie zależy od popularności.
- **Z filtrami** korzysta z wyszukiwarki `/search/repositories`, która obsługuje
  wszystkie filtry po stronie GitHuba. Program pobiera losową stronę wyników
  i wybiera losowe repozytorium z tej strony. Wyszukiwarka udostępnia tylko
  pierwsze 1000 wyników, więc bardzo szerokie zapytania próbkują ten zakres.

W obu trybach wybór jest zawsze losowy.

## Licencja

MIT — zobacz [LICENSE](LICENSE).
