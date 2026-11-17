//! 邮件正文的轻量排版：把邮件拆成「标题 / 段落 / 列表 / 引用 / 分隔线」，
//! 让阅读窗口不再是一整段纯文本。不引入 HTML 渲染引擎，只保留能提升可读性的结构。

use crate::mail;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 正文里的大标题
    Title,
    /// 小标题
    Heading,
    Body,
    Bullet,
    /// 有序列表项，文本自带 `1. ` 这样的序号
    Numbered,
    Quote,
    /// 段落之间的空隙（不是一条线）
    Gap,
    /// 真正的分隔线
    Divider,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
    pub rich: bool,
}

/// 正文排版行。HTML 原文优先；纯图片/空壳 HTML 会退回 text/plain，
/// 避免营销邮件只剩一片空白。两者都按「实际文字量」择优。
pub fn lines(html: &str, text: &str) -> Vec<Line> {
    let html = clean(html);
    let text = clean(text);
    let plain = if text.trim().is_empty() { Vec::new() } else { from_text(&text) };
    let rich = if html.trim().is_empty() { Vec::new() } else { from_html(&html) };
    let score = |v: &[Line]| -> usize {
        v.iter()
            .filter(|l| !matches!(l.kind, Kind::Gap | Kind::Divider))
            .map(|l| l.text.chars().count())
            .sum()
    };
    // 只在 HTML 明显是空壳（纯图片、只剩追踪像素）时回退；结构化排版本身
    // 会丢掉标签与空白，短邮件里字符数天然略少于 text/plain，不能用 >= 比较。
    if rich.is_empty() || score(&rich) * 2 < score(&plain) { plain } else { rich }
}

/// 零宽空格、零宽连接符、软连字符都不占位，留着只会让断行和复制变得古怪。
fn clean(text: &str) -> std::borrow::Cow<'_, str> {
    let noisy = |c: char| matches!(c, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}' | '\u{00ad}');
    if text.contains(noisy) {
        std::borrow::Cow::Owned(text.chars().filter(|c| !noisy(*c)).collect())
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// 列表项与阅读窗口共用。真实 HTML 邮件必须走 `from_html`：
/// multipart 邮件的 text/plain 版本通常已把标题、列表和段落全部压平。
pub fn item_lines(item: &mail::Item) -> Vec<Line> {
    lines(&item.html, &item.summary)
}

fn from_text(text: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut gap = false;
    for raw in text.replace("\r\n", "\n").lines() {
        let line = raw.trim();
        if line.is_empty() {
            gap = true;
            continue;
        }
        let (kind, body) = split_marked(line);
        if kind == Kind::Divider {
            if !out.is_empty() && !matches!(out.last(), Some(Line { kind: Kind::Divider, .. })) {
                out.push(Line { kind: Kind::Divider, text: String::new(), rich:false });
            }
            gap = false;
            continue;
        }
        let body = body.trim();
        if body.is_empty() {
            gap = true;
            continue;
        }
        // 同一种列表的相邻项之间不额外留白，列表切换或段落之间才留一点。
        let list_run = matches!(
            (kind, out.last().map(|l| l.kind)),
            (Kind::Bullet, Some(Kind::Bullet)) | (Kind::Numbered, Some(Kind::Numbered))
        );
        if gap && !list_run && !out.is_empty() && !matches!(out.last(), Some(Line { kind: Kind::Gap | Kind::Divider, .. })) {
            out.push(Line { kind: Kind::Gap, text: String::new(), rich:false });
        }
        gap = false;
        out.push(Line { kind, text: body.to_string(), rich:false });
    }
    while matches!(out.last(), Some(Line { kind: Kind::Gap | Kind::Divider, .. })) {
        out.pop();
    }
    out
}

/// 行首标记 → 块类型。`## ` 标题、`• ` 列表、`1. ` 有序列表、`> ` 引用、`---` 分隔线。
fn split_marked(line: &str) -> (Kind, &str) {
    if let Some(rest) = line.strip_prefix("## ") {
        (Kind::Heading, rest)
    } else if let Some(rest) = line.strip_prefix("• ") {
        (Kind::Bullet, rest)
    } else if is_numbered(line) {
        (Kind::Numbered, line)
    } else if let Some(rest) = line.strip_prefix("> ") {
        (Kind::Quote, rest)
    } else if let Some(rest) = line.strip_prefix("# ") {
        (Kind::Title, rest)
    } else if line == "---" {
        (Kind::Divider, "")
    } else {
        (Kind::Body, line)
    }
}

/// `1. ` / `12. ` 这类有序列表前缀。位数限三位，免得把 `2026.10 版本` 当成列表。
fn is_numbered(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && digits <= 3 && line[digits..].starts_with(". ")
}

pub(crate) fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        let end = tail.as_bytes().iter().take(11).position(|&byte| byte == b';');
        let Some(end) = end else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        let decoded = match entity {
            "nbsp" | "#160" | "ensp" | "emsp" | "thinsp" | "#8194" | "#8195" | "#8201" => Some(' '),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "middot" | "#183" | "bull" | "#8226" => Some('·'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "copy" => Some('©'),
            "reg" => Some('®'),
            // 零宽字符与软连字符交给 `clean` 统一清除，这里先映射成实体本身。
            "zwnj" => Some('\u{200c}'),
            "zwj" => Some('\u{200d}'),
            "shy" => Some('\u{00ad}'),
            other => other
                .strip_prefix("#x")
                .or_else(|| other.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| other.strip_prefix('#').and_then(|dec| dec.parse::<u32>().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 布局表格：明确声明 presentation，或「零间距 + 没有可见边框」。
/// 只看开始标签的属性，免得带 cellpadding 的真实数据表格被当成排版外壳。
fn is_layout_table(attrs: &str) -> bool {
    use std::sync::OnceLock;
    static BORDER: OnceLock<regex::Regex> = OnceLock::new();
    static STYLE_BORDER: OnceLock<regex::Regex> = OnceLock::new();
    static ZERO_GAP: OnceLock<regex::Regex> = OnceLock::new();
    let lower = attrs.to_ascii_lowercase();
    if lower.contains("presentation") {
        return true;
    }
    let border = BORDER.get_or_init(|| regex::Regex::new(r#"(?i)\bborder\s*=\s*["']?([0-9]+)"#).expect("border"));
    let visible = border
        .captures(&lower)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .is_some_and(|width| width > 0)
        || STYLE_BORDER.get_or_init(|| regex::Regex::new(r"(?i)\bborder\s*:\s*[1-9]").expect("style"))
            .is_match(&lower);
    if visible {
        return false;
    }
    ZERO_GAP
        .get_or_init(|| regex::Regex::new(r#"(?i)\b(?:cellpadding|cellspacing)\s*=\s*["']?0\b"#).expect("gap"))
        .is_match(&lower)
}

/// 图片的 `alt` 只有在真正描述内容时才值得占一行；`logo`、`icon`、追踪像素
/// 这类装饰性替代文字会让正文多出一串噪音。
fn image_alt(tag: &str) -> Option<String> {
    use std::sync::OnceLock;
    static ALT: OnceLock<regex::Regex> = OnceLock::new();
    let alt = ALT.get_or_init(|| regex::Regex::new(r#"(?is)\balt\s*=\s*["']([^"']*)["']"#).expect("alt"));
    let text = alt.captures(tag)?.get(1)?.as_str().trim().to_string();
    let lower = text.to_ascii_lowercase();
    let noise = ["logo", "icon", "spacer", "pixel", "banner", "avatar", "image", "图片", "图标", "头像", "配图"];
    if text.chars().count() < 2 || noise.iter().any(|word| lower == *word) {
        None
    } else {
        Some(text)
    }
}

/// 按元素层级跳过隐藏块，避免嵌套标签提前结束隐藏范围。
pub(crate) fn visible_html(html:&str)->String {
    use std::sync::OnceLock;
    static TAG:OnceLock<regex::Regex>=OnceLock::new();
    static HIDDEN:OnceLock<regex::Regex>=OnceLock::new();
    let tag=TAG.get_or_init(||regex::Regex::new(r#"(?is)<\s*(/?)\s*([a-z][a-z0-9:-]*)\b((?:"[^"]*"|'[^']*'|[^'">])*)>"#).unwrap());
    let hidden=HIDDEN.get_or_init(||regex::Regex::new(r#"(?is)(?:^|\s)hidden(?:\s|=|/|$)|\bstyle\s*=\s*["'][^"']*(?:display\s*:\s*none|mso-hide\s*:\s*all|visibility\s*:\s*hidden)[^"']*["']"#).unwrap());
    let mut out=String::with_capacity(html.len());let mut stack:Vec<String>=Vec::new();let mut cursor=0;
    for cap in tag.captures_iter(html){
        let hit=cap.get(0).unwrap();let name=cap[2].to_ascii_lowercase();let attrs=&cap[3];
        if stack.is_empty(){out.push_str(&html[cursor..hit.start()]);}
        if &cap[1]=="/" {
            if let Some(at)=stack.iter().rposition(|n|n==&name){stack.truncate(at);}
            else if stack.is_empty(){out.push_str(hit.as_str());}
        }else{
            let skip=!stack.is_empty()||hidden.is_match(attrs);
            if !skip{out.push_str(hit.as_str());}
            if skip&&!attrs.trim_end().ends_with('/')&&!matches!(name.as_str(),"area"|"base"|"br"|"col"|"embed"|"hr"|"img"|"input"|"link"|"meta"|"param"|"source"|"track"|"wbr"){stack.push(name);}
        }
        cursor=hit.end();
    }
    if stack.is_empty(){out.push_str(&html[cursor..]);}out
}

fn from_html(html: &str) -> Vec<Line> {
    use std::sync::OnceLock;
    static COMMENT: OnceLock<regex::Regex> = OnceLock::new();
    static LAYOUT: OnceLock<regex::Regex> = OnceLock::new();
    static SCRIPT: OnceLock<regex::Regex> = OnceLock::new();
    static ORDERED: OnceLock<regex::Regex> = OnceLock::new();
    static ORDERED_ITEM: OnceLock<regex::Regex> = OnceLock::new();
    static MARK: OnceLock<regex::Regex> = OnceLock::new();
    static STRONG: OnceLock<regex::Regex> = OnceLock::new();
    static HEADER_CELL: OnceLock<regex::Regex> = OnceLock::new();
    static IMAGE: OnceLock<regex::Regex> = OnceLock::new();
    static CELL: OnceLock<regex::Regex> = OnceLock::new();
    static HARD: OnceLock<regex::Regex> = OnceLock::new();
    static SOFT: OnceLock<regex::Regex> = OnceLock::new();
    static TAGS: OnceLock<regex::Regex> = OnceLock::new();

    // 注释里可能塞着整段 Outlook 条件代码（按钮文案会因此出现两次），整段丢弃。
    let comment = COMMENT.get_or_init(|| regex::Regex::new(r"(?s)<!--.*?-->").expect("comment"));
    // 用 table 做排版（而不是装数据）时，单元格之间不能插「·」，否则会串成一句。
    // 判断依据放在闭包里：既看 role="presentation"，也看「零间距且没有可见边框」，
    // 否则带 cellpadding 的真实数据表格会被误伤。
    let layout = LAYOUT.get_or_init(|| regex::Regex::new(r"(?is)<table\b([^>]*)>(.*?)</table\s*>").expect("layout"));
    let script = SCRIPT.get_or_init(|| {
        regex::Regex::new(r"(?is)<(?:script|style|head)[^>]*>.*?</(?:script|style|head)>").expect("script")
    });
    // 有序列表按出现顺序编号；ol 之外的 li 仍然走项目符号。
    let ordered = ORDERED.get_or_init(|| regex::Regex::new(r"(?is)<ol\b[^>]*>(.*?)</ol\s*>").expect("ol"));
    let ordered_item = ORDERED_ITEM.get_or_init(|| regex::Regex::new(r"(?is)<li\b[^>]*>").expect("li"));
    let mark = MARK.get_or_init(|| {
        regex::Regex::new(r"(?is)<hr\s*/?>|<h[12][^>]*>|<h[3-6][^>]*>|<li[^>]*>|<blockquote[^>]*>").expect("mark")
    });
    let strong = STRONG.get_or_init(|| regex::Regex::new(r"(?i)</?(?:strong|b)\b[^>]*>").expect("strong"));
    // 表头和数据单元格一样按「·」分隔，但保留粗体以便和正文区分。
    let header_cell = HEADER_CELL.get_or_init(|| regex::Regex::new(r"(?i)<th\b[^>]*>|</th\s*>").expect("th"));
    // 图片本身渲染不了，只把有信息量的 alt 留成占位，装饰图直接丢掉。
    let image = IMAGE.get_or_init(|| regex::Regex::new(r"(?is)<img\b[^>]*>").expect("img"));
    let cell = CELL.get_or_init(|| regex::Regex::new(r"(?i)</t[dh]>").expect("cell"));
    let hard = HARD.get_or_init(|| {
        regex::Regex::new(r"(?i)</p>|</h[1-6]>|</blockquote>|</table>|</ul>|</ol>|</pre>").expect("hard")
    });
    let soft = SOFT.get_or_init(|| regex::Regex::new(r"(?i)<br\s*/?>|</div>|</tr>|</li>").expect("soft"));
    let tags = TAGS.get_or_init(|| regex::Regex::new(r"(?s)<[^>]+>").expect("tag"));

    let s = comment.replace_all(html, " ");
    let s = visible_html(&s);
    let s = layout.replace_all(&s, |caps: &regex::Captures| {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        if !is_layout_table(attrs) {
            return caps.get(0).map(|m| m.as_str()).unwrap_or_default().to_string();
        }
        let inner = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
        cell.replace_all(inner, "\n").into_owned()
    });
    let s = script.replace_all(&s, " ");
    let s = ordered.replace_all(&s, |caps: &regex::Captures| {
        let inner = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let mut nth = 0usize;
        ordered_item
            .replace_all(inner, |_: &regex::Captures| {
                nth += 1;
                format!("\n{nth}. ")
            })
            .into_owned()
    });
    let s = mark.replace_all(&s, |caps: &regex::Captures| {
        let raw = caps.get(0).map(|m| m.as_str()).unwrap_or_default().to_ascii_lowercase();
        if raw.starts_with("<hr") {
            "\n\n---\n\n".to_string()
        } else if raw.starts_with("<h1") || raw.starts_with("<h2") {
            "\n\n# ".to_string()
        } else if raw.starts_with("<h") {
            "\n\n## ".to_string()
        } else if raw.starts_with("<li") {
            "\n• ".to_string()
        } else {
            "\n> ".to_string()
        }
    });
    let s = strong.replace_all(&s, "**");
    let s = image.replace_all(&s, |caps: &regex::Captures| {
        let tag = caps.get(0).map(|m| m.as_str()).unwrap_or_default();
        image_alt(tag).map(|alt| format!("[图片：{alt}]")).unwrap_or_default()
    });
    let s = header_cell.replace_all(&s, |caps: &regex::Captures| {
        if caps.get(0).is_some_and(|m| m.as_str().starts_with("</")) { "** · " } else { "**" }
    });
    let s = cell.replace_all(&s, " · ");
    let s = hard.replace_all(&s, "\n\n");
    let s = soft.replace_all(&s, "\n");
    let s = tags.replace_all(&s, "");
    let s = decode(&s);
    let s = clean(&s);

    let mut out: Vec<Line> = Vec::new();
    let mut gap = false;
    for raw in s.split('\n') {
        let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = joined.trim().trim_end_matches('·').trim();
        if line.is_empty() {
            gap = true;
            continue;
        }
        let (kind, body) = split_marked(line);
        if kind == Kind::Divider {
            if !out.is_empty() && !matches!(out.last(), Some(Line { kind: Kind::Divider, .. })) {
                out.push(Line { kind: Kind::Divider, text: String::new(), rich:true });
            }
            gap = false;
            continue;
        }
        let list_run = matches!(
            (kind, out.last().map(|l| l.kind)),
            (Kind::Bullet, Some(Kind::Bullet)) | (Kind::Numbered, Some(Kind::Numbered))
        );
        if gap && !list_run && !out.is_empty() && !matches!(out.last(), Some(Line { kind: Kind::Gap | Kind::Divider, .. })) {
            out.push(Line { kind: Kind::Gap, text: String::new(), rich:true });
        }
        gap = false;
        out.push(Line { kind, text: body.to_string(), rich:true });
    }
    while matches!(out.last(), Some(Line { kind: Kind::Gap | Kind::Divider, .. })) {
        out.pop();
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_many_unterminated_entities_is_bounded(){
        let text=format!("{}中文&#x10ffff;&amp;", "&".repeat(250_000));
        let start=std::time::Instant::now();
        let decoded=decode(&text);
        assert!(start.elapsed()<std::time::Duration::from_secs(2));
        assert_eq!(decoded,format!("{}中文{}&","&".repeat(250_000),'\u{10ffff}'));
    }

    #[test]
    fn nested_hidden_blocks_never_leak_into_the_body() {
        let out=lines("<div style=\"display:none\"><span>预览</span><p>隐藏验证码 123456</p></div><p>正式正文</p>","");
        assert_eq!(out.iter().map(|l|l.text.as_str()).collect::<Vec<_>>(),["正式正文"]);
    }

    #[test]
    fn plain_text_keeps_literal_asterisks_and_paragraphs() {
        let out=lines("","保留 ** 原始字符\n\n第二段");
        assert_eq!(out[0].text,"保留 ** 原始字符");
        assert_eq!(out[1].kind,Kind::Gap);
    }

    #[test]
    fn html_headings_lists_and_quotes_keep_their_kind() {
        let html = r#"<html><head><style>p{color:red}</style></head><body>
            <h1>本周周报</h1><p>完成 <b>三件事</b>：</p>
            <ul><li>修复登录</li><li>同步版本</li></ul>
            <blockquote>注意：周五前提交</blockquote><hr><p>谢谢</p></body></html>"#;
        let lines = lines(html, "");
        assert_eq!(lines[0].kind, Kind::Title);
        assert_eq!(lines[0].text, "本周周报");
        assert!(lines.iter().any(|l| l.kind == Kind::Bullet && l.text == "修复登录"));
        let quote=lines.iter().find(|l| l.kind == Kind::Quote).expect("应有一行引用");
        assert_eq!(quote.text, "注意：周五前提交");
        assert!(lines.iter().any(|l| l.kind == Kind::Divider));
        assert!(!lines.iter().any(|l| l.text.contains("color:red")), "style 应被丢弃");
    }

    #[test]
    fn entities_are_decoded() {
        let lines = lines("<p>价格 &lt; 100 &amp; 包邮&nbsp;·&nbsp;限今天</p>", "");
        assert_eq!(lines[0].text, "价格 < 100 & 包邮 · 限今天");
    }

    #[test]
    fn plain_text_marks_become_blocks() {
        let text = "## 今日摘要\n第一段\n\n• 事项一\n• 事项二\n\n> 提醒：明天截止\n---\n最后一行";
        let lines = lines("", text);
        assert_eq!(lines[0].kind, Kind::Heading);
        assert_eq!(lines[1].kind, Kind::Body);
        assert_eq!(lines[2].kind, Kind::Gap);
        assert_eq!(lines[3].kind, Kind::Bullet);
        assert!(lines.iter().any(|l| l.kind == Kind::Quote && l.text.starts_with("提醒")));
        assert!(lines.iter().any(|l| l.kind == Kind::Divider));
        assert_eq!(lines.last().unwrap().text, "最后一行");
    }

    #[test]
    fn real_newsletter_html_keeps_structure_and_bold() {
        let html = r#"<html><head><style>p{color:red}</style></head><body>
            <h1>订单确认</h1>
            <p>您好，<strong>订单已支付</strong>，明细如下：</p>
            <table><tr><td>商品</td><td>金额</td></tr><tr><td>会员</td><td>¥99</td></tr></table>
            <ul><li>发货时间：明天</li><li>发票：电子</li></ul>
            <blockquote>如有疑问请联系客服</blockquote><hr><p>感谢支持</p></body></html>"#;
        let out = lines(html, "订单确认 您好 订单已支付");
        assert_eq!(out[0].kind, Kind::Title, "h1 应是大标题");
        assert!(out.iter().any(|l| l.text.contains("**订单已支付**")), "strong 应保留粗体标记");
        assert!(out.iter().any(|l| l.text == "商品 · 金额"), "表格单元格应分隔而不是粘连");
        assert_eq!(out.iter().filter(|l| l.kind == Kind::Bullet).count(), 2);
        assert!(out.iter().any(|l| l.kind == Kind::Quote));
        assert!(out.iter().any(|l| l.kind == Kind::Divider));
        assert!(!out.iter().any(|l| l.text.contains("color:red")));
    }

    #[test]
    fn preheader_and_conditional_comments_do_not_reach_the_body() {
        let html = r#"<html><body>
            <div style="display:none;max-height:0;overflow:hidden">&#8203;&#8203; 这是一段只为收件箱预览存在的填充文字 &#8203;</div>
            <!--[if mso]><table><tr><td><b>打开网页版</b></td></tr></table><![endif]-->
            <p>真正的正文。</p></body></html>"#;
        let out = lines(html, "");
        assert_eq!(out.len(), 1, "隐藏块与条件注释都不应变成正文：{out:?}");
        assert_eq!(out[0].text, "真正的正文。");
    }

    #[test]
    fn ordered_list_numbers_survive_conversion() {
        let out = lines("<ol><li>先备份</li><li>再升级</li><li>最后验证</li></ol>", "");
        let numbered: Vec<_> = out.iter().filter(|l| l.kind == Kind::Numbered).map(|l| l.text.as_str()).collect();
        assert_eq!(numbered, ["1. 先备份", "2. 再升级", "3. 最后验证"]);
    }

    #[test]
    fn data_table_keeps_columns_and_bolds_its_header() {
        let html = r#"<table border="1" cellpadding="6" cellspacing="0">
            <tr><th>项目</th><th>数值</th></tr><tr><td>未读</td><td>12</td></tr></table>"#;
        let out = lines(html, "");
        assert!(out.iter().any(|l| l.text.contains("**项目**")), "表头应加粗：{out:?}");
        assert!(out.iter().any(|l| l.text.contains("未读 · 12")), "数据表格的列不能被拆散：{out:?}");
    }

    #[test]
    fn layout_table_is_flattened_instead_of_joined_by_dots() {
        let html = r#"<table role="presentation" cellpadding="0" cellspacing="0"><tr>
            <td><img src="logo.png"></td><td>首页</td><td>关于我们</td></tr></table>"#;
        let out = lines(html, "");
        assert!(!out.iter().any(|l| l.text.contains('·')), "布局表格不该串成一行：{out:?}");
        assert!(out.iter().any(|l| l.text.contains("首页")));
    }

    #[test]
    fn zero_width_padding_is_cleaned() {
        let out = lines("<p>验证码\u{200b}已\u{200b}发送</p>", "");
        assert_eq!(out[0].text, "验证码已发送");
        let plain = lines("", "第一行\u{feff}\n\n第二行");
        assert_eq!(plain[0].text, "第一行");
    }

    #[test]
    fn images_keep_only_meaningful_alt_text() {
        let html = r#"<p>订单截图如下：<img src="order.png" alt="订单截图"></p>
            <p><img src="logo.png" alt="logo"><img src="pixel.gif" alt=""></p>"#;
        let out = lines(html, "");
        assert!(out.iter().any(|l| l.text == "订单截图如下：[图片：订单截图]"), "{out:?}");
        assert!(!out.iter().any(|l| l.text.to_ascii_lowercase().contains("logo")), "装饰图不该留下噪音：{out:?}");
    }

    #[test]
    fn image_only_html_falls_back_to_plain_text() {
        let html = r#"<html><body><img src="cid:a"><img src="cid:b"></body></html>"#;
        let out = lines(html, "尊敬的客户，您的账单已生成。");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "尊敬的客户，您的账单已生成。");
    }

    #[test]
    fn item_lines_uses_html_even_when_plain_text_exists() {
        let item = mail::Item {
            account: "a@qq.com".into(), from: "团队".into(), subject: "周报".into(),
            time: "09:41".into(), code: None, received_unix: None,
            summary: "本周周报 完成三件事".into(),
            html: "<h2>本周周报</h2><ul><li>完成三件事</li></ul>".into(),
            links: vec![], meeting: None, uid: 7,
        };
        let out = item_lines(&item);
        assert_eq!(out[0].kind, Kind::Title, "不能退化成压平的 text/plain");
        assert!(out.iter().any(|l| l.kind == Kind::Bullet && l.text == "完成三件事"));
    }

    #[test]
    fn structures_free_html_stays_flat() {
        let lines = lines("<div>一</div><div>二</div><div>三</div>", "");
        assert_eq!(lines.iter().filter(|l| l.kind == Kind::Body).count(), 3);
        assert!(!lines.iter().any(|l| l.kind == Kind::Divider));
    }
}
