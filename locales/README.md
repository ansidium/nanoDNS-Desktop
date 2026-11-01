# GUI translations

The GUI includes 30 languages: English (`en`), Russian (`ru`), German (`de`),
French (`fr`), Spanish (`es`), Italian (`it`), Brazilian Portuguese (`pt-BR`),
European Portuguese (`pt-PT`), Dutch (`nl`), Polish (`pl`), Ukrainian (`uk`),
Czech (`cs`), Slovak (`sk`), Turkish (`tr`), Greek (`el`), Hungarian (`hu`),
Romanian (`ro`), Bulgarian (`bg`), Croatian (`hr`), Danish (`da`), Swedish (`sv`),
Norwegian Bokmål (`nb`), Finnish (`fi`), Indonesian (`id`), Vietnamese (`vi`),
Simplified Chinese (`zh-Hans`), Traditional Chinese (`zh-Hant`), Japanese (`ja`),
Korean (`ko`), and Malay (`ms`). Arabic is not included.

Each UTF-8 JSON object has the same 184 English source keys: 154 fixed labels
and 30 complete message templates. English keys are stable identifiers, and
`en.json` repeats each key as its value. Preserve `{}` placeholders in their
original order and preserve line breaks. Save all keys and translations in
Unicode NFC. Run `python scripts/check-locales.py` after changing any catalog.
Add new keys to the Russian source mapping and every catalog in the same change.

Windows selects the GUI language from `GetUserDefaultUILanguage`, including the
Brazilian and European Portuguese variants. Unsupported languages fall back to
English. Croatian detection distinguishes Croatian from Serbian/Bosnian, which
share its Windows primary language ID. Norwegian Windows variants use Bokmål.
Malay display languages in Malaysia and Brunei use the shared `ms` catalog.

For QA, set the process environment variable `NANODNS_LANGUAGE` before starting
the app, for example `fr-CA`, `pt_BR`, or `VI`. Tags are case insensitive, `_`
and `-` are accepted, and regional variants use their supported base language.
Bare `pt` and Portuguese regions other than Brazil use European Portuguese;
`no` is an alias for `nb`. Unsupported overrides leave Windows autodetection in
effect. The selection is fixed for that process and does not change settings.
The application currently supports Windows only.

Chinese tags honor explicit `Hans`/`Hant` scripts first; `zh-CN` and `zh-SG`
select Simplified Chinese, while `zh-TW`, `zh-HK`, and `zh-MO` select Traditional
Chinese. Bare `zh` selects Simplified Chinese. Windows detection distinguishes
the same regions and recognizes Japanese and Korean UI languages.

All catalogs are embedded in the executable. The selected non-English catalog is
parsed on first use and cached. Missing or malformed translations fall back to
English; repository validation rejects both cases. Russian display source maps
to English keys before lookup. Paths, IP addresses, domains, network names, settings, DNS packets, and
unknown strings are preserved verbatim. Complete message templates substitute
captured values; only templates explicitly marked as containing nested app
errors translate their captures, with a bounded recursion depth.
