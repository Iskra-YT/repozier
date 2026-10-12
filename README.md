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

OPTIONS:
    -n, --count <N>          Ile repozytoriów wypisać [default: 1]
    -l, --language <LANG>    Filtruj po języku (np. rust, python, go)
        --min-stars <N>      Minimalna liczba gwiazdek [default: 0]
        --no-forks           Pomijaj forki
        --no-archived        Pomijaj zarchiwizowane
        --no-empty           Pomijaj puste repozytoria (bez zawartości)
        --has-description    Tylko repozytoria z opisem
        --json               Wypisz wynik jako JSON
        --open               Otwórz link w przeglądarce
        --max-id <N>         Górna granica ID (zaawansowane)
        --token <TOKEN>      Token GitHub (domyślnie z GITHUB_TOKEN)
    -h, --help               Pomoc
    -V, --version            Wersja
```

### Przykłady

Losowe repo w Rust z co najmniej 100 gwiazdkami:

```console
$ repozier --language rust --min-stars 100
https://github.com/... 
```

Trzy losowe repozytoria w Pythonie, bez forków:

```console
$ repozier -n 3 --language python --no-forks
```

Repozytorium z rzeczywistą zawartością (pomijając puste):

```console
$ repozier --no-empty
```

Wynik jako JSON (np. do dalszego przetwarzania):

```console
$ repozier --json
{"full_name":"owner/name","url":"https://github.com/owner/name", ...}
```

## Token GitHub

Bez tokenu działa publiczne API GitHuba z limitem ok. 60 zapytań na godzinę.
Dla wyższego limitu ustaw token (nie musi mieć żadnych uprawnień, wystarczy
publiczny dostęp do odczytu):

```console
$ export GITHUB_TOKEN=ghp_xxx
$ repozier
```

Token można też podać flagą `--token`, ale zmienna środowiskowa jest
bezpieczniejsza (nie trafia do historii powłoki).

## Jak losuje

`repozier` korzysta z endpointu `/repositories?since=<id>`, który zwraca
publiczne repozytoria posortowane po ID. Losuje ID, pobiera stronę wyników,
a następnie wybiera losowe repo spełniające podane filtry. Dzięki temu
rozklad jest w praktyce równomierny i nie wymaga zgadywania liczby stron.

## Licencja

MIT — zobacz [LICENSE](LICENSE).
