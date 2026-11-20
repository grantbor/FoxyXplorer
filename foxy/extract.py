import os
import re
import json

STRING_PATTERN = re.compile(r'(?<![rRbB])"((?:[^"\\]|\\.)*)"')
CYRILLIC_PATTERN = re.compile(r'[а-яА-ЯёЁ]')

def remove_comments(content):
    """
    Удаляет комментарии из Rust-кода, сохраняя строки.
    Обрабатывает: //, /* */, ///, //!
    """
    result = []
    i = 0
    in_string = False
    string_char = None
    in_line_comment = False
    in_block_comment = False
    
    while i < len(content):
        # Обработка однострочного комментария
        if in_line_comment:
            if content[i] == '\n':
                in_line_comment = False
                result.append(content[i])
            i += 1
            continue
        
        # Обработка многострочного комментария
        if in_block_comment:
            if i + 1 < len(content) and content[i:i+2] == '*/':
                in_block_comment = False
                i += 2
            else:
                i += 1
            continue
        
        # Обработка строки
        if in_string:
            result.append(content[i])
            if content[i] == '\\':
                # Пропускаем экранированный символ
                if i + 1 < len(content):
                    result.append(content[i+1])
                    i += 2
                else:
                    i += 1
            elif content[i] == string_char:
                in_string = False
                i += 1
            else:
                i += 1
            continue
        
        # Проверка на начало строки
        if content[i] == '"':
            # Проверяем, что это не r"..." или b"..."
            if i > 0 and content[i-1] in 'rRbB':
                result.append(content[i])
                i += 1
                continue
            
            in_string = True
            string_char = '"'
            result.append(content[i])
            i += 1
            continue
        
        # Проверка на начало однострочного комментария
        if i + 1 < len(content) and content[i:i+2] == '//':
            in_line_comment = True
            i += 2
            continue
        
        # Проверка на начало многострочного комментария
        if i + 1 < len(content) and content[i:i+2] == '/*':
            in_block_comment = True
            i += 2
            continue
        
        # Обычный символ
        result.append(content[i])
        i += 1
    
    return ''.join(result)

def extract_strings(src_dir, output_json):
    unique_strings = []
    
    # 1. Сбор всех строк
    for root, _, files in os.walk(src_dir):
        # Исключаем папки с зависимостями и таргет-директории
        if 'target' in root or '.git' in root:
            continue
            
        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                with open(filepath, 'r', encoding='utf-8') as f:
                    content = f.read()
                
                # Удаляем комментарии перед поиском строк
                content_no_comments = remove_comments(content)
                
                for match in STRING_PATTERN.finditer(content_no_comments):
                    text = match.group(1)
                    # Пропускаем строки с форматированием, чтобы не сломать format!
                    if '{' in text or '}' in text:
                        continue
                        
                    if CYRILLIC_PATTERN.search(text):
                        if text not in unique_strings:
                            unique_strings.append(text)

    # 2. Формирование JSON
    translations = {}
    for i, text in enumerate(unique_strings):
        # Генерируем читаемый ключ (можно заменить на транслитерацию, если нужно)
        key = f"msg_{i+1:04d}" 
        translations[key] = {
            "ru": text,
            "en": ""  # Заполните переводы здесь (вручную или скриптом через API)
        }

    with open(output_json, 'w', encoding='utf-8') as f:
        json.dump(translations, f, ensure_ascii=False, indent=2)
        
    print(f"Найдено {len(unique_strings)} уникальных строк. Сохранено в {output_json}")

if __name__ == "__main__":
    # Укажите путь к src вашему проекту
    extract_strings('./src', 'translations.json')