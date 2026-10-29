//! «Переписать с ИИ»: тот же сервер и тот же промпт, что в мобильном приложении.

const URL: &str = "https://fptools.onrender.com/api/ai";

pub struct Ctx {
    pub history: String,
    pub username: String,
    pub chat_id: String,
    pub lot_name: String,
    pub last_message: String,
}

pub fn rewrite(text: &str, c: &Ctx) -> Result<String, String> {
    let mut context = String::new();
    if !c.history.trim().is_empty() {
        context.push_str("\n--- ИСТОРИЯ ПЕРЕПИСКИ ---\n");
        context.push_str(&c.history);
        context.push_str("\n--- КОНЕЦ ИСТОРИИ ---\n");
    }
    if !c.username.is_empty() {
        context.push_str(&format!("\nИмя покупателя: {}", c.username));
    }
    if !c.chat_id.is_empty() {
        context.push_str(&format!("\nID чата: {}", c.chat_id));
    }
    if !c.lot_name.is_empty() {
        context.push_str(&format!("\nЛот: {}", c.lot_name));
    }
    if !c.last_message.is_empty() {
        context.push_str(&format!("\nПоследнее сообщение покупателя: {}", c.last_message));
    }
    let prompt = format!(
        "Ты — ИИ-ассистент, который помогает продавцу на FunPay. Твоя задача — переписать его черновик сообщения, сохранив основной смысл, но сделав его вежливым, профессиональным и четким.\n\n\
--- ОСНОВНЫЕ ПРАВИЛА ---\n\
1.  СОХРАНЯЙ СМЫСЛ: Твой ответ должен передавать ТОТ ЖЕ САМЫЙ смысл, что и черновик продавца. Не добавляй новые идеи, вопросы или предложения от себя.\n\
2.  БУДЬ КРАТОК: Ответ должен быть настолько же коротким, насколько позволяет исходное сообщение. Не пиши длинные тексты, если черновик короткий.\n\
3.  ДЕЙСТВУЙ ОТ ЛИЦА ПРОДАВЦА: Всегда пиши от имени продавца.\n\
4.  УЧИТЫВАЙ КОНТЕКСТ: Изучи историю переписки, чтобы твой ответ был уместен.\n\
5.  СТИЛЬ: Используй вежливый, но уверенный тон.\n\
6.  НИКАКИХ ЛИШНИХ СЛОВ: Не добавляй стандартные фразы вроде \"Здравствуйте\", если их не было в исходном черновике или они неуместны.\n\
7.  ТОЛЬКО ТЕКСТ: Твой итоговый ответ — это ТОЛЬКО готовый текст сообщения. Без кавычек, заголовков (типа \"Вот ваш текст\") или объяснений.\n\
{context}\n\n\
ЧЕРНОВИК МОЕГО СООБЩЕНИЯ (от продавца): \"{text}\"\n\n\
ПЕРЕПИШИ МОЙ ЧЕРНОВИК, СТРОГО СЛЕДУЯ ВСЕМ ПРАВИЛАМ.\n\
ГОТОВЫЙ ТЕКСТ:"
    );
    let body = serde_json::json!({
        "messages": [
            { "role": "system", "content": "You are a text editing model. Follow user instructions precisely." },
            { "role": "user", "content": prompt }
        ],
        "modelName": "ChatGPT 4o",
        "currentPagePath": "/chatgpt-4o"
    });
    let client = crate::fp::http_client_follow(90);
    // сервер на бесплатном хостинге может просыпаться, поэтому одна повторная попытка
    let mut last_err = String::new();
    for attempt in 0..2 {
        let r = client
            .post(URL)
            .header("Content-Type", "application/json")
            .header("Authorization", "Bearer fptoolsdim")
            .body(body.to_string())
            .send();
        match r {
            Ok(r) => {
                let code = r.status();
                let txt = r.text().unwrap_or_default();
                if code.is_success() {
                    let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or_default();
                    let out = v
                        .get("response")
                        .and_then(|x| x.as_str())
                        .or_else(|| v.get("result").and_then(|x| x.as_str()))
                        .unwrap_or("")
                        .trim()
                        .trim_matches('"')
                        .trim()
                        .to_string();
                    if !out.is_empty() {
                        return Ok(out);
                    }
                    last_err = "ИИ вернул пустой ответ".into();
                } else {
                    crate::log::write(&format!("ИИ: {code} {}", txt.chars().take(200).collect::<String>()));
                    last_err = format!("ИИ сейчас недоступен ({})", code.as_u16());
                }
            }
            Err(e) => {
                last_err = if e.is_timeout() { "ИИ не ответил вовремя".into() } else { "Нет связи с сервером ИИ".into() };
            }
        }
        if attempt == 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
    Err(last_err)
}
