//! Мои лоты: список со своей страницы, форма редактирования (lots/offerEdit) и сохранение (lots/offerSave).
//! Разбор формы повторяет FunPay Cardinal: только поля внутри form.form-offer-editor, textarea с переносами,
//! условные поля из data-fields, чекбоксы с hidden-двойниками.

use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

fn text(e: ElementRef) -> String {
    e.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MyLot {
    pub id: String,
    pub title: String,
    pub node: String,
    pub category: String,
    pub price: String,
    pub amount: String,
    pub server: String,
    pub active: bool,
    pub auto: bool,
}

static S_OFFER: Lazy<Selector> = Lazy::new(|| sel(".offer"));
static S_OFFER_TITLE: Lazy<Selector> = Lazy::new(|| sel(".offer-list-title a"));
static S_ROW: Lazy<Selector> = Lazy::new(|| sel("a.tc-item"));
static S_DESC: Lazy<Selector> = Lazy::new(|| sel(".tc-desc-text"));
static S_PRICE: Lazy<Selector> = Lazy::new(|| sel(".tc-price"));
static S_UNIT: Lazy<Selector> = Lazy::new(|| sel(".unit"));
static S_SERVER: Lazy<Selector> = Lazy::new(|| sel(".tc-server"));
static S_SIDE: Lazy<Selector> = Lazy::new(|| sel(".tc-side"));
static S_AMOUNT: Lazy<Selector> = Lazy::new(|| sel(".tc-amount"));
static S_AUTO: Lazy<Selector> = Lazy::new(|| sel(".auto-dlv-icon"));
static RE_NODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"/(?:lots|chips)/(\d+)").unwrap());
static RE_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:offer=|id=)([0-9A-Za-z\-]+)").unwrap());

/// Лоты со своей страницы профиля (FunPay показывает там только активные).
pub fn parse_my_lots(html: &str) -> Vec<MyLot> {
    let doc = Html::parse_document(html);
    let mut out = Vec::new();
    for block in doc.select(&S_OFFER) {
        let Some(cat) = block.select(&S_OFFER_TITLE).next() else { continue };
        let category = text(cat);
        let href = cat.value().attr("href").unwrap_or("");
        let Some(node) = RE_NODE.captures(href).map(|c| c[1].to_string()) else { continue };
        for row in block.select(&S_ROW) {
            let href = row.value().attr("href").unwrap_or("");
            let Some(id) = RE_ID.captures(href).map(|c| c[1].to_string()) else { continue };
            let price_el = row.select(&S_PRICE).next();
            let price = price_el
                .map(|p| {
                    let unit = p.select(&S_UNIT).next().map(text).unwrap_or_default();
                    let num = p.value().attr("data-s").map(|s| s.to_string()).unwrap_or_else(|| text(p).replace(&unit, "").trim().to_string());
                    format!("{} {}", num, unit).trim().to_string()
                })
                .unwrap_or_default();
            let server = [row.select(&S_SERVER).next().map(text), row.select(&S_SIDE).next().map(text)]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            out.push(MyLot {
                id,
                title: row.select(&S_DESC).next().map(text).filter(|s| !s.is_empty()).unwrap_or_else(|| "Без названия".into()),
                node: node.clone(),
                category: category.clone(),
                price,
                amount: row.select(&S_AMOUNT).next().map(text).unwrap_or_default(),
                server,
                active: !row.value().classes().any(|c| c == "warning"),
                auto: row.select(&S_AUTO).next().is_some(),
            });
        }
    }
    out
}

// ---------------------------------------------------------------- форма лота

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Field {
    pub name: String,
    /// text | number | textarea | select | checkbox | hidden
    pub kind: String,
    pub value: String,
    pub label: String,
    pub options: Vec<(String, String)>,
    pub locale: String,
    /// (имя поля, допустимые значения): показывать, только если все условия выполнены
    pub conds: Vec<(String, Vec<String>)>,
    pub hint: String,
    pub placeholder: String,
    /// у чекбокса есть hidden-двойник: выключенный отправляется пустым
    pub empty_off: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Form {
    pub fields: Vec<Field>,
    pub csrf: String,
    pub currency: String,
    pub db_amount: Option<i64>,
    pub images: usize,
}

impl Form {
    pub fn get(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn value(&self, name: &str) -> String {
        self.get(name).map(|f| f.value.clone()).unwrap_or_default()
    }

    pub fn set(&mut self, name: &str, v: &str) {
        match self.fields.iter_mut().find(|f| f.name == name) {
            Some(f) => f.value = v.to_string(),
            None => self.fields.push(Field { name: name.into(), kind: "hidden".into(), value: v.into(), ..Default::default() }),
        }
    }

    /// Показывается ли поле при текущих значениях (условные поля категории).
    pub fn visible(&self, f: &Field) -> bool {
        f.conds.iter().all(|(name, vals)| {
            let cur = self.value(name).trim().to_lowercase();
            vals.iter().any(|v| v.trim().to_lowercase() == cur)
        })
    }

    /// Тело offerSave так, как его отправил бы браузер.
    pub fn body(&self) -> Vec<(String, String)> {
        let mut out = vec![("csrf_token".to_string(), self.csrf.clone())];
        for f in &self.fields {
            if f.name == "csrf_token" || !self.visible(f) {
                continue;
            }
            match f.kind.as_str() {
                "checkbox" => {
                    if f.empty_off {
                        out.push((f.name.clone(), String::new()));
                    }
                    if matches!(f.value.as_str(), "on" | "true" | "1") {
                        out.push((f.name.clone(), "on".into()));
                    }
                }
                "number" => out.push((f.name.clone(), f.value.replace(',', ".").replace([' ', '\u{a0}'], ""))),
                _ => out.push((f.name.clone(), f.value.clone())),
            }
        }
        out
    }
}

static S_FORM: Lazy<Selector> = Lazy::new(|| sel("form.form-offer-editor"));
static S_LEAD: Lazy<Selector> = Lazy::new(|| sel("p.lead"));
static S_INPUTS: Lazy<Selector> = Lazy::new(|| sel("input[name], textarea[name], select[name]"));
static S_OPTION: Lazy<Selector> = Lazy::new(|| sel("option"));
static S_LABEL_CTL: Lazy<Selector> = Lazy::new(|| sel("label.control-label"));
static S_LABEL: Lazy<Selector> = Lazy::new(|| sel("label"));
static S_HELP: Lazy<Selector> = Lazy::new(|| sel(".help-block"));
static S_FIELDS_BOX: Lazy<Selector> = Lazy::new(|| sel("div.lot-fields[data-fields]"));
static S_FEEDBACK: Lazy<Selector> = Lazy::new(|| sel(".form-control-feedback"));
static S_IMAGES: Lazy<Selector> = Lazy::new(|| sel("li.attachments-item[data-file-id]"));
static S_BODY: Lazy<Selector> = Lazy::new(|| sel("body"));

fn has_class(e: &ElementRef, c: &str) -> bool {
    e.value().classes().any(|x| x == c)
}

fn ancestors(e: ElementRef) -> impl Iterator<Item = ElementRef> {
    e.ancestors().filter_map(ElementRef::wrap)
}

pub fn parse_form(html: &str) -> Result<Form, String> {
    let doc = Html::parse_document(html);
    let Some(form) = doc.select(&S_FORM).next() else {
        if let Some(lead) = doc.select(&S_LEAD).next() {
            let t = text(lead);
            if !t.is_empty() {
                return Err(t);
            }
        }
        return Err(if html.contains("account/login") { "FunPay не пустил: войдите в аккаунт заново".into() } else { "Форма лота не найдена".into() });
    };
    let mut csrf = String::new();
    // условия показа полей
    let mut conds_by_id: std::collections::HashMap<String, Vec<(String, Vec<String>)>> = Default::default();
    for b in form.select(&S_FIELDS_BOX) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(b.value().attr("data-fields").unwrap_or("[]")) else { continue };
        for obj in v.as_array().cloned().unwrap_or_default() {
            let id = obj.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let mut list = Vec::new();
            for c in obj.get("conditions").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
                let cid = c.get("id").map(|x| x.to_string().trim_matches('"').to_string()).unwrap_or_default();
                let vals = c.get("list").and_then(|l| l.as_array()).map(|a| a.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect()).unwrap_or_default();
                list.push((format!("fields[{cid}]"), vals));
            }
            if !id.is_empty() && !list.is_empty() {
                conds_by_id.insert(id, list);
            }
        }
    }
    let checkbox_names: std::collections::HashSet<String> = form
        .select(&S_INPUTS)
        .filter(|e| e.value().attr("type").map(|t| t.eq_ignore_ascii_case("checkbox")).unwrap_or(false))
        .filter_map(|e| e.value().attr("name").map(String::from))
        .collect();
    let mut fields: Vec<Field> = Vec::new();
    let mut twins = std::collections::HashSet::new();
    for el in form.select(&S_INPUTS) {
        let v = el.value();
        let name = v.attr("name").unwrap_or("").to_string();
        if name.is_empty() || name == "query" || name.starts_with("cc-option") {
            continue;
        }
        if name == "csrf_token" {
            csrf = v.attr("value").unwrap_or("").to_string();
            continue;
        }
        let group = ancestors(el).find(|a| has_class(a, "form-group") || has_class(a, "lot-field"));
        let field_id = ancestors(el).find(|a| has_class(a, "lot-field") && a.value().attr("data-id").is_some()).and_then(|a| a.value().attr("data-id").map(String::from));
        let conds = field_id.and_then(|id| conds_by_id.get(&id).cloned()).unwrap_or_default();
        let locale = group
            .and_then(|g| g.value().attr("data-locale").map(String::from))
            .filter(|s| !s.is_empty())
            .or_else(|| if name.ends_with("[ru]") { Some("ru".into()) } else if name.ends_with("[en]") { Some("en".into()) } else { None })
            .unwrap_or_default();
        let label = group.and_then(|g| g.select(&S_LABEL_CTL).next().or_else(|| g.select(&S_LABEL).next())).map(text).unwrap_or_default();
        let hint = group.and_then(|g| g.select(&S_HELP).next()).map(text).unwrap_or_default();
        let base = Field { name: name.clone(), label: label.clone(), locale, conds, hint, ..Default::default() };
        match v.name() {
            "textarea" => {
                let val: String = el.text().collect();
                let val = val.strip_prefix("\r\n").or_else(|| val.strip_prefix('\n')).unwrap_or(&val).to_string();
                fields.push(Field { kind: "textarea".into(), value: val, label: if label.is_empty() { name.clone() } else { label }, ..base });
            }
            "select" => {
                let opts: Vec<(String, String)> = el
                    .select(&S_OPTION)
                    .map(|o| (o.value().attr("value").unwrap_or("").to_string(), text(o).replace('\u{a0}', " ").trim().to_string()))
                    .collect();
                let val = el.select(&S_OPTION).find(|o| o.value().attr("selected").is_some()).and_then(|o| o.value().attr("value").map(String::from)).unwrap_or_default();
                fields.push(Field { kind: "select".into(), value: val, options: opts, label: if label.is_empty() { name.clone() } else { label }, ..base });
            }
            _ => {
                let ty = v.attr("type").unwrap_or("text").to_ascii_lowercase();
                match ty.as_str() {
                    "submit" | "button" | "file" | "image" | "reset" => {}
                    "hidden" => {
                        if fields.iter().any(|f| f.name == name) {
                            continue;
                        }
                        if checkbox_names.contains(&name) {
                            twins.insert(name.clone());
                            continue;
                        }
                        fields.push(Field { kind: "hidden".into(), value: v.attr("value").unwrap_or("").into(), ..base });
                    }
                    "checkbox" => {
                        let lbl = el.parent().and_then(ElementRef::wrap).filter(|p| p.value().name() == "label").map(text).filter(|s| !s.is_empty()).unwrap_or(if label.is_empty() { name.clone() } else { label });
                        fields.retain(|f| f.name != name);
                        fields.push(Field {
                            kind: "checkbox".into(),
                            value: if v.attr("checked").is_some() { "on".into() } else { String::new() },
                            label: lbl,
                            empty_off: twins.contains(&name),
                            ..base
                        });
                    }
                    "radio" => {
                        let opt_label = el.parent().and_then(ElementRef::wrap).map(text).filter(|s| !s.is_empty()).unwrap_or_else(|| v.attr("value").unwrap_or("").into());
                        let opt = (v.attr("value").unwrap_or("").to_string(), opt_label);
                        let checked = v.attr("checked").is_some();
                        match fields.iter_mut().find(|f| f.name == name) {
                            Some(f) => {
                                f.options.push(opt.clone());
                                if checked {
                                    f.value = opt.0;
                                }
                            }
                            None => fields.push(Field { kind: "select".into(), value: if checked { opt.0.clone() } else { String::new() }, options: vec![opt], ..base }),
                        }
                    }
                    _ => {
                        let number = name == "price" || name == "amount" || ty == "number" || v.attr("inputmode") == Some("decimal");
                        let lbl = match name.as_str() {
                            "amount" if label.is_empty() => "Наличие".into(),
                            "price" if label.is_empty() => "Цена".into(),
                            _ if label.is_empty() => name.clone(),
                            _ => label,
                        };
                        fields.push(Field {
                            kind: if number { "number".into() } else { "text".into() },
                            value: v.attr("value").unwrap_or("").into(),
                            label: lbl,
                            placeholder: v.attr("placeholder").unwrap_or("").into(),
                            ..base
                        });
                    }
                }
            }
        }
    }
    // hidden-двойник мог встретиться после чекбокса
    for f in fields.iter_mut() {
        if f.kind == "checkbox" && twins.contains(&f.name) {
            f.empty_off = true;
        }
    }
    if csrf.is_empty() {
        csrf = doc
            .select(&S_BODY)
            .next()
            .and_then(|b| b.value().attr("data-app-data"))
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|v| v.get("csrf-token").and_then(|x| x.as_str()).map(String::from))
            .unwrap_or_default();
    }
    let db_amount = form
        .value()
        .attr("data-offer")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("amount").and_then(|a| a.as_i64().or_else(|| a.as_str().and_then(|s| s.parse().ok()))));
    Ok(Form {
        fields,
        csrf,
        currency: form.select(&S_FEEDBACK).next().map(text).unwrap_or_default(),
        db_amount,
        images: form.select(&S_IMAGES).count(),
    })
}

/// Подпись поля для редактора: русское имя, отметка языка.
pub fn nice_label(f: &Field) -> String {
    let mut l = match f.name.as_str() {
        "fields[summary][ru]" | "fields[summary][en]" if f.label.starts_with("fields") => "Краткое описание".to_string(),
        "fields[desc][ru]" | "fields[desc][en]" if f.label.starts_with("fields") => "Подробное описание".to_string(),
        "fields[payment_msg][ru]" | "fields[payment_msg][en]" if f.label.starts_with("fields") => "Сообщение покупателю после оплаты".to_string(),
        "active" => "Активный лот".to_string(),
        "deactivate_after_sale" => "Деактивировать после продажи".to_string(),
        "auto_delivery" => "Автовыдача".to_string(),
        _ => f.label.clone(),
    };
    if l.is_empty() {
        l = f.name.clone();
    }
    match f.locale.as_str() {
        "ru" => format!("{l} (RU)"),
        "en" => format!("{l} (EN)"),
        _ => l,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_and_conditions() {
        let html = r#"<body data-app-data='{"csrf-token":"x"}'><form class="form-offer-editor" data-offer='{"amount":5}'>
<input type="hidden" name="csrf_token" value="tok"><input type="hidden" name="offer_id" value="42"><input type="hidden" name="node_id" value="7">
<div class="form-group lot-field" data-id="type"><label class="control-label">Тип</label><select name="fields[type]"><option value="">-</option><option value="Реакции" selected>Реакции</option></select></div>
<div class="form-group lot-field" data-id="react"><label class="control-label">Тип реакции</label><input type="text" name="fields[react]" value="лайк"></div>
<div class="lot-fields" data-fields='[{"id":"react","conditions":[{"id":"type","list":["Реакции"]}]}]'></div>
<div class="form-group" data-locale="ru"><label class="control-label">Описание</label><textarea name="fields[desc][ru]">
строка 1
строка 2</textarea></div>
<input type="text" name="price" value="100"><span class="form-control-feedback">₽</span>
<input type="hidden" name="active" value=""><label><input type="checkbox" name="active" checked> Активный</label>
</form></body>"#;
        let mut f = parse_form(html).unwrap();
        assert_eq!(f.csrf, "tok");
        assert_eq!(f.db_amount, Some(5));
        assert_eq!(f.value("fields[desc][ru]"), "строка 1\nстрока 2");
        let react = f.get("fields[react]").unwrap().clone();
        assert!(f.visible(&react));
        let body = f.body();
        assert!(body.contains(&("active".into(), "".into())) && body.contains(&("active".into(), "on".into())));
        f.set("fields[type]", "");
        assert!(!f.visible(&react));
        assert!(!f.body().iter().any(|(k, _)| k == "fields[react]"));
        assert_eq!(f.get("price").unwrap().kind, "number");
    }
}
