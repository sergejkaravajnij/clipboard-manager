use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    Russian,
}
thread_local! { static LANGUAGE: Cell<Language> = const { Cell::new(Language::English) }; }
pub fn set(language: Language) {
    LANGUAGE.with(|l| l.set(language));
}
pub fn current() -> Language {
    LANGUAGE.with(Cell::get)
}
pub fn tr(en: &str, ru: &str) -> String {
    current().choose(en, ru).into()
}
impl Language {
    pub fn choose<'a>(self, en: &'a str, ru: &'a str) -> &'a str {
        match self {
            Self::English => en,
            Self::Russian => ru,
        }
    }
    pub fn code(self) -> &'static str {
        self.choose("en", "ru")
    }
    pub fn generated(self, text: &str) -> String {
        if let Some(rest) = text
            .strip_prefix("Изображение ")
            .or_else(|| text.strip_prefix("Image "))
        {
            format!("{} {rest}", self.choose("Image", "Изображение"))
        } else if let Some(rest) = text
            .strip_prefix("Данные: ")
            .or_else(|| text.strip_prefix("Data: "))
        {
            format!("{}: {rest}", self.choose("Data", "Данные"))
        } else {
            text.into()
        }
    }
}

pub fn message(text: &str) -> String {
    if current() == Language::Russian {
        let mut text = text.to_string();
        for (en, ru) in [
            ("Invalid shortcut", "Недопустимое сочетание клавиш"),
            (
                "This key cannot be used as a shortcut",
                "Эта клавиша не может использоваться как хоткей",
            ),
            (
                "Add Ctrl, Alt, Shift or Windows to the shortcut",
                "Добавьте к хоткею Ctrl, Alt, Shift или Windows",
            ),
            (
                "This shortcut is already in use or reserved by Windows. Choose another combination.",
                "Сочетание уже занято или зарезервировано Windows. Выберите другое.",
            ),
            (
                "Could not initialize settings controls",
                "Не удалось инициализировать элементы настроек",
            ),
            (
                "Could not register settings window",
                "Не удалось зарегистрировать окно настроек",
            ),
            (
                "Could not create settings window",
                "Не удалось создать окно настроек",
            ),
            (
                "Could not create settings controls",
                "Не удалось создать элементы настроек",
            ),
            (
                "Invalid language in settings.ini",
                "Недопустимый язык в settings.ini",
            ),
        ] {
            text = text.replace(en, ru);
        }
        return text;
    }
    let mut text = text.to_string();
    for (ru, en) in [
        (
            "Буфер обмена занят другим приложением",
            "The clipboard is busy in another application",
        ),
        (
            "Owner-display: данные принадлежат окну исходного приложения",
            "Owner-display: the source application owns this data",
        ),
        (
            "Виртуальные файлы не входят в поддерживаемые форматы",
            "Virtual files are not supported",
        ),
        (
            "ссылки на объекты процесса не переносятся в историю",
            "process object references cannot be saved in history",
        ),
        (
            "Не удалось прочитать формат ",
            "Could not read clipboard format ",
        ),
        (
            "Не удалось прочитать метафайл",
            "Could not read the metafile",
        ),
        ("Ошибка чтения метафайла", "Could not read the metafile"),
        (
            "Не удалось прочитать METAFILEPICT",
            "Could not read METAFILEPICT",
        ),
        ("Ошибка чтения METAFILEPICT", "Could not read METAFILEPICT"),
        ("Ошибка чтения палитры", "Could not read the palette"),
        ("Формат ", "Format "),
        (
            " не содержит переносимых данных",
            " does not contain portable data",
        ),
        (
            "Не удалось заблокировать данные буфера",
            "Could not lock clipboard data",
        ),
        ("Повреждённый метафайл", "The metafile is corrupt"),
        (
            "Ошибка восстановления метафайла",
            "Could not restore the metafile",
        ),
        ("Недостаточно памяти", "Not enough memory"),
        (
            "Ошибка восстановления формата буфера",
            "Could not restore the clipboard format",
        ),
        ("Запись пуста", "This entry is empty"),
        ("Повреждённая запись", "This entry is corrupt"),
        ("Не удалось зарегистрировать ", "Could not register "),
        (
            "Не удалось очистить буфер обмена",
            "Could not clear the clipboard",
        ),
        ("Не удалось восстановить ", "Could not restore "),
        (
            "Не удалось открыть автозапуск: ",
            "Could not open startup settings: ",
        ),
        (
            "Не удалось изменить автозапуск: ",
            "Could not change startup settings: ",
        ),
        ("LOCALAPPDATA отсутствует", "LOCALAPPDATA is missing"),
        (
            "Не удалось зарегистрировать окно",
            "Could not register the window",
        ),
        ("Не удалось создать окно", "Could not create the window"),
        (
            "Не удалось создать элементы окна",
            "Could not create window controls",
        ),
        (
            "Не удалось добавить значок в трей",
            "Could not add the tray icon",
        ),
        (
            "Не удалось подписаться на изменения буфера",
            "Could not subscribe to clipboard changes",
        ),
        (
            "Не удалось создать блокировку приложения",
            "Could not create the application lock",
        ),
        ("Ошибка цикла сообщений", "Message loop failed"),
        ("Не сохранено: ", "Not saved: "),
    ] {
        text = text.replace(ru, en);
    }
    text
}

pub fn preview(text: &str, generated: bool) -> String {
    if let Some((body, formats)) = text.rsplit_once("\r\n\r\nФорматы: ") {
        let body = if generated {
            current().generated(body)
        } else {
            body.into()
        };
        format!(
            "{body}\r\n\r\n{}: {}",
            tr("Formats", "Форматы"),
            message(formats)
        )
    } else if generated {
        current().generated(text)
    } else {
        text.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translates_metadata_without_translating_copied_text() {
        set(Language::English);
        assert_eq!(
            preview(
                "Изображение is copied text\r\n\r\nФорматы: Unicode text",
                false
            ),
            "Изображение is copied text\r\n\r\nFormats: Unicode text"
        );
        assert_eq!(
            preview("Изображение 10 × 20\r\n\r\nФорматы: DIB image", true),
            "Image 10 × 20\r\n\r\nFormats: DIB image"
        );
        set(Language::Russian);
        assert_eq!(
            Language::Russian.generated("Image 10 × 20"),
            "Изображение 10 × 20"
        );
    }
}
