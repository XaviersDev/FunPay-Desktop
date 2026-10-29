//! Описания профилей FunPay Tools. В самом FunPay описаний нет: тексты лежат на сервере
//! FunPay Tools, общем для расширения, мобильного приложения и этой программы.

use serde_json::Value;

const BASE: &str = "https://fpt-descs.starobinskiy01.workers.dev";
const KEY: &str = "fptoolsdim";

fn client() -> reqwest::blocking::Client {
    crate::fp::http_client_follow(20)
}

fn err_of(v: &Value, status: u16) -> String {
    v.get("error")
        .or_else(|| v.get("message"))
        .or_else(|| v.get("msg"))
        .and_then(|e| e.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Сервер описаний ответил ошибкой (HTTP {status})"))
}

fn send(rb: reqwest::blocking::RequestBuilder) -> Result<(u16, Value), String> {
    let r = rb.header("X-FPT-Key", KEY).send().map_err(|e| {
        if e.is_timeout() {
            "Сервер описаний долго не отвечает".to_string()
        } else {
            "Нет связи с сервером описаний".to_string()
        }
    })?;
    let status = r.status().as_u16();
    let text = r.text().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok((status, v))
}

#[derive(Debug, Clone, Default)]
pub struct Desc {
    pub text: String,
    pub banner: String,
}

/// Описание и баннер пользователя (своего или чужого).
pub fn get(uid: &str) -> Result<Desc, String> {
    let (status, v) = send(client().get(format!("{BASE}/funpay/users/{uid}/profile")))?;
    if status >= 400 {
        return Err(err_of(&v, status));
    }
    Ok(Desc {
        text: v.get("description").and_then(|x| x.as_str()).unwrap_or("").trim().to_string(),
        banner: v.get("bannerUrl").and_then(|x| x.as_str()).unwrap_or("").trim().to_string(),
    })
}

/// Шаг 1: код подтверждения для лота.
pub fn link_start(uid: &str) -> Result<String, String> {
    let id: i64 = uid.parse().map_err(|_| "Не знаю ваш ID на FunPay".to_string())?;
    let (status, v) = send(client().post(format!("{BASE}/me/funpay/link/start")).json(&serde_json::json!({ "funpayUserId": id })))?;
    if status >= 400 {
        return Err(err_of(&v, status));
    }
    v.get("code").and_then(|c| c.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty()).ok_or_else(|| err_of(&v, status))
}

/// Шаг 3: сервер ищет лот с кодом. Some(session), если нашёл.
pub fn link_confirm(uid: &str) -> Result<Option<String>, String> {
    let id: i64 = uid.parse().map_err(|_| "Не знаю ваш ID на FunPay".to_string())?;
    let (status, v) = send(client().post(format!("{BASE}/me/funpay/link/confirm")).json(&serde_json::json!({ "funpayUserId": id })))?;
    if status >= 500 {
        return Err(err_of(&v, status));
    }
    if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false) {
        if let Some(s) = v.get("session").and_then(|s| s.as_str()).filter(|s| !s.is_empty()) {
            return Ok(Some(s.to_string()));
        }
    }
    Ok(None)
}

/// Шаг 5: сохранить описание. Err("auth") значит, что сессия протухла.
pub fn put(session: &str, text: &str) -> Result<(), String> {
    let (status, v) = send(
        client()
            .put(format!("{BASE}/me/funpay/description"))
            .header("Authorization", format!("Bearer {session}"))
            .json(&serde_json::json!({ "description": text })),
    )?;
    if status == 401 || status == 403 {
        return Err("auth".into());
    }
    if status >= 400 || v.get("ok").and_then(|o| o.as_bool()) == Some(false) {
        return Err(err_of(&v, status));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn live_desc_get() {
        let d = super::get("2846982").expect("get");
        println!("{d:?}");
    }
}
