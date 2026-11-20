#!/usr/bin/env python3
"""
replace.py — заменяет русские строковые литералы в .rs на вызовы t!("msg_XXXX").

Использование:
    python3 replace.py                       # dry-run: отчёт, файлы не меняются
    python3 replace.py --apply               # применить изменения
    python3 replace.py --apply --add-missing # + добавить новые ключи в translations.json
    python3 replace.py --stats               # только цифры

Особенности:
- Пропускает технические сообщения (println!, eprintln!, dbg!, panic!, assert!*,
  todo!, unimplemented!, unreachable!, write!, writeln!, format_args!, concat!,
  stringify!, env!, option_env!, include!, include_str!, include_bytes!).
- Пропускает форматные строки (с {} или {name}) — они требуют ручного
  разбиения на t!("prefix") + аргументы. Все такие места собираются в отчёт.
- Пропускает комментарии // и /* */.
- Пропускает raw- и byte-строки (r"...", r#"..."#, b"...", br"...").
- Автоматически вставляет `use crate::t;` в модифицированные файлы, если его нет.
- Идемпотентен: повторный запуск ничего не ломает.
- Декодирует escape-последовательности Rust для корректного сравнения с JSON
  (важно для строк с \\n — в JSON это "\\n", в исходнике "\\n" или реальный перевод строки).
- Не меняет translations.json без флага --add-missing.

Требует:
- src/i18n.rs с публичным макросом t! (возвращает String, чтобы не ломать
  места, где нужен String — например, self.status).
- mod i18n; в main.rs.
"""

import argparse
import json
import re
import sys
from pathlib import Path

# Строковый литерал; lookbehind отсекает r/R/b/B и # (raw-строки r"..." и r#"...").
STRING_LITERAL = re.compile(r'(?<![rRbB#])"((?:[^"\\]|\\.)*)"', re.DOTALL)

CYRILLIC = re.compile(r'[а-яА-ЯёЁ]')
FORMAT_PLACEHOLDER = re.compile(r'\{[^}]*\}')

SKIP_MACROS = frozenset([
    'println', 'eprintln', 'print', 'eprint',
    'dbg', 'panic', 'assert', 'assert_eq', 'assert_ne',
    'debug_assert', 'debug_assert_eq', 'debug_assert_ne',
    'write', 'writeln',
    'todo', 'unimplemented', 'unreachable',
    'include_str', 'include_bytes', 'include',
    'env', 'option_env',
    'concat', 'stringify', 'format_args',
])


def decode_rust_string(raw: str) -> str:
    """Декодирует \\n, \\t, \\", \\\\, \\xNN, \\u{NNNN} → runtime-строку.
    Неизвестные escape оставляет как есть."""
    out, i, n = [], 0, len(raw)
    while i < n:
        c = raw[i]
        if c != '\\' or i + 1 >= n:
            out.append(c); i += 1; continue
        nxt = raw[i + 1]
        if nxt == 'n':   out.append('\n'); i += 2
        elif nxt == 't': out.append('\t'); i += 2
        elif nxt == 'r': out.append('\r'); i += 2
        elif nxt == '0': out.append('\0'); i += 2
        elif nxt == '\\': out.append('\\'); i += 2
        elif nxt == '"':  out.append('"'); i += 2
        elif nxt == "'":  out.append("'"); i += 2
        elif nxt == 'x' and i + 3 < n:
            try:
                out.append(chr(int(raw[i+2:i+4], 16))); i += 4
            except ValueError:
                out.append(c); i += 1
        elif nxt == 'u' and i + 2 < n and raw[i+2] == '{':
            end = raw.find('}', i+3)
            if end > 0:
                try:
                    out.append(chr(int(raw[i+3:end], 16))); i = end + 1
                except ValueError:
                    out.append(c); i += 1
            else:
                out.append(c); i += 1
        else:
            out.append(c); i += 1
    return ''.join(out)


def is_in_comment(content: str, pos: int) -> bool:
    before = content[:pos]
    if before.count('/*') > before.count('*/'):
        return True
    line_start = before.rfind('\n') + 1
    line = before[line_start:]
    # Строковые литералы в line заменяем, чтобы `http://...` не считался комментарием.
    line_clean = STRING_LITERAL.sub('""', line)
    return '//' in line_clean


def find_macro_context(content: str, string_start: int) -> str | None:
    """Возвращает имя макроса, если строка — его аргумент верхнего уровня."""
    i, depth = string_start - 1, 0
    while i >= 0:
        c = content[i]
        if c == ')':
            depth += 1
        elif c == '(':
            if depth == 0:
                j = i - 1
                while j >= 0 and content[j] in ' \t\n':
                    j -= 1
                if j >= 0 and content[j] == '!':
                    k = j - 1
                    while k >= 0 and content[k] in ' \t\n':
                        k -= 1
                    end = k + 1
                    while k >= 0 and (content[k].isalnum() or content[k] == '_'):
                        k -= 1
                    return content[k+1:end]
                return None
            depth -= 1
        elif c in ';{':
            return None
        i -= 1
    return None


def find_use_insert_point(content: str) -> int:
    matches = list(re.finditer(r'^use\s+[^;]+;', content, re.MULTILINE))
    if matches:
        return matches[-1].end()
    attrs = list(re.finditer(r'^#!\[[^\]]+\]', content, re.MULTILINE))
    if attrs:
        return attrs[-1].end()
    return 0


def has_use_t(content: str) -> bool:
    if re.search(r'^use\s+crate::t\s*;', content, re.MULTILINE):
        return True
    if re.search(r'^use\s+crate::i18n::t\s*;', content, re.MULTILINE):
        return True
    for m in re.finditer(r'^use\s+crate::\{([^}]+)\}\s*;', content, re.MULTILINE):
        if re.search(r'\bt\b', m.group(1)):
            return True
    return False


def ensure_use_t(content: str) -> str:
    if has_use_t(content):
        return content
    pos = find_use_insert_point(content)
    return content[:pos] + '\nuse crate::t;' + content[pos:]


def next_key(translations: dict) -> str:
    max_n = 0
    for k in translations:
        m = re.match(r'msg_(\d+)$', k)
        if m:
            max_n = max(max_n, int(m.group(1)))
    return f'msg_{max_n + 1:04d}'


def process_file(path: Path, translations: dict, ru_to_key: dict,
                 add_missing: bool, apply: bool, report: dict, src_root: Path) -> None:
    try:
        content = path.read_text(encoding='utf-8')
    except UnicodeDecodeError:
        print(f'⚠ {path}: не UTF-8, пропущен', file=sys.stderr)
        return

    replacements = []
    skipped_format, skipped_comment, skipped_macro, missing = [], [], [], []
    new_entries = []

    for match in STRING_LITERAL.finditer(content):
        raw = match.group(1)
        if not CYRILLIC.search(raw):
            continue

        decoded = decode_rust_string(raw)

        if is_in_comment(content, match.start()):
            skipped_comment.append(decoded)
            continue

        macro = find_macro_context(content, match.start())
        if macro and macro in SKIP_MACROS:
            skipped_macro.append((decoded, macro))
            continue

        if FORMAT_PLACEHOLDER.search(decoded):
            skipped_format.append(decoded)
            continue

        key = ru_to_key.get(decoded)
        if key is None:
            if add_missing:
                key = next_key(translations)
                translations[key] = {'ru': decoded, 'en': decoded}
                ru_to_key[decoded] = key
                new_entries.append((decoded, key))
            else:
                missing.append(decoded)
                continue

        replacements.append((match.start(0), match.end(0), f't!("{key}")'))

    if not (replacements or new_entries):
        return

    new_content = content
    for start, end, repl in sorted(replacements, key=lambda x: -x[0]):
        new_content = new_content[:start] + repl + new_content[end:]
    new_content = ensure_use_t(new_content)

    if apply and new_content != content:
        path.write_text(new_content, encoding='utf-8')

    rel = path.relative_to(src_root)
    report['files_modified'].append(str(rel))
    report['replacements_count'] += len(replacements)
    report['new_entries'].extend((str(rel), t, k) for t, k in new_entries)
    report['skipped_format'].extend((str(rel), s) for s in skipped_format)
    report['skipped_comment'].extend((str(rel), s) for s in skipped_comment)
    report['skipped_macro'].extend((str(rel), s, m) for s, m in skipped_macro)
    report['missing'].extend((str(rel), s) for s in missing)


def print_report(report: dict, stats_only: bool) -> None:
    print(f"\n=== Отчёт ===")
    print(f"Файлов изменено:  {len(report['files_modified'])}")
    print(f"Заменено строк:    {report['replacements_count']}")
    print(f"Новых ключей:      {len(report['new_entries'])}")
    print(f"Пропущено (формат): {len(report['skipped_format'])}")
    print(f"Пропущено (нет перевода): {len(report['missing'])}")
    print(f"Пропущено (технические): {len(report['skipped_macro'])}")
    print(f"Пропущено (комментарии): {len(report['skipped_comment'])}")

    if stats_only:
        return

    if report['files_modified']:
        print("\nФайлы:")
        for f in report['files_modified']:
            print(f"  {f}")

    if report['new_entries']:
        print(f"\nНовые ключи ({len(report['new_entries'])}):")
        for path, text, key in report['new_entries']:
            preview = text[:60].replace('\n', '\\n')
            print(f"  {key} <- '{preview}'  ({path})")

    if report['missing']:
        print(f"\n⚠ Кириллические строки БЕЗ перевода (пропущены): {len(report['missing'])}")
        for path, text in report['missing']:
            preview = text[:60].replace('\n', '\\n')
            print(f"  '{preview}'  ({path})")

    if report['skipped_format']:
        print(f"\n⚠ Форматные строки (пропущены, нужно разбить вручную): {len(report['skipped_format'])}")
        for path, text in report['skipped_format']:
            preview = text[:80].replace('\n', '\\n')
            print(f"  '{preview}'  ({path})")

    if report['skipped_macro']:
        print(f"\nℹ Технические сообщения (пропущены): {len(report['skipped_macro'])}")
        for path, text, macro in report['skipped_macro'][:8]:
            preview = text[:50].replace('\n', '\\n')
            print(f"  {macro}!('{preview}')  ({path})")
        if len(report['skipped_macro']) > 8:
            print(f"  ... и ещё {len(report['skipped_macro']) - 8}")


def main():
    p = argparse.ArgumentParser(description='Rust i18n string replacer')
    p.add_argument('--src', default='./src')
    p.add_argument('--translations', default='./assets/translations.json')
    p.add_argument('--apply', action='store_true',
                   help='применить (по умолчанию dry-run)')
    p.add_argument('--add-missing', action='store_true',
                   help='добавлять новые ключи для строк без перевода')
    p.add_argument('--stats', action='store_true',
                   help='только цифры, без подробного списка')
    args = p.parse_args()

    src_dir = Path(args.src)
    trans_path = Path(args.translations)

    if not src_dir.is_dir():
        print(f'❌ Не найден каталог {src_dir}', file=sys.stderr); sys.exit(1)
    if not trans_path.is_file():
        print(f'❌ Не найден {trans_path}', file=sys.stderr); sys.exit(1)

    with trans_path.open(encoding='utf-8') as f:
        translations = json.load(f)
    ru_to_key = {v['ru']: k for k, v in translations.items()}

    report = {
        'files_modified': [], 'replacements_count': 0,
        'new_entries': [], 'missing': [],
        'skipped_format': [], 'skipped_comment': [], 'skipped_macro': [],
    }

    for rs_path in src_dir.rglob('*.rs'):
        if any(part in ('target', '.git') for part in rs_path.parts):
            continue
        process_file(rs_path, translations, ru_to_key,
                     args.add_missing, args.apply, report, src_dir)

    if args.apply and report['new_entries']:
        sorted_trans = dict(sorted(translations.items()))
        with trans_path.open('w', encoding='utf-8') as f:
            json.dump(sorted_trans, f, ensure_ascii=False, indent=2)
            f.write('\n')
        print(f'\n✓ translations.json: +{len(report["new_entries"])} новых ключей')

    print_report(report, args.stats)

    if not args.apply:
        print(f"\nℹ Dry-run. Для применения: --apply")


if __name__ == '__main__':
    main()
