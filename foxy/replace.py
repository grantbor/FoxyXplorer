import os
import re
import json

STRING_PATTERN = re.compile(r'(?<![rRbB])"((?:[^"\\]|\\.)*)"')
CYRILLIC_PATTERN = re.compile(r'[а-яА-ЯёЁ]')

def replace_strings(src_dir, translations_json, macro_name="t"):
    # Загружаем переводы и создаем обратный словарь: "русский текст" -> "ключ"
    with open(translations_json, 'r', encoding='utf-8') as f:
        translations = json.load(f)
        
    ru_to_key = {data["ru"]: key for key, data in translations.items()}

    files_modified = 0
    
    for root, _, files in os.walk(src_dir):
        if 'target' in root or '.git' in root:
            continue
            
        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                with open(filepath, 'r', encoding='utf-8') as f:
                    content = f.read()

                new_content = []
                last_idx = 0
                changed = False

                for match in STRING_PATTERN.finditer(content):
                    text = match.group(1)
                    
                    # Если это кириллица и мы знаем для неё ключ
                    if CYRILLIC_PATTERN.search(text) and text in ru_to_key:
                        key = ru_to_key[text]
                        
                        # Формируем замену: t!("msg_0001")
                        replacement = f'{macro_name}!("{key}")'
                        
                        new_content.append(content[last_idx:match.start()])
                        new_content.append(replacement)
                        last_idx = match.end()
                        changed = True

                if changed:
                    new_content.append(content[last_idx:])
                    with open(filepath, 'w', encoding='utf-8') as f:
                        f.write("".join(new_content))
                    files_modified += 1

    print(f"Модифицировано файлов: {files_modified}")

if __name__ == "__main__":
    # macro_name="t" подойдет для популярного крейта rust-i18n
    # Если используете другой крейт, измените на "tr", "i18n", etc.
    replace_strings('./src', 'translations.json', macro_name="t")