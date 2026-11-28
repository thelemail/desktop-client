import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire, register } from 'node:module';

const hooks = `
export async function resolve(specifier, context, next) {
	if (specifier === '$paraglide/messages.js') {
		return { url: 'data:text/javascript,export const m = new Proxy({}, { get: (_, k) => () => String(k) });', shortCircuit: true };
	}
	if (specifier === '$core/i18n/locale.svelte') {
		return { url: 'data:text/javascript,export const i18n = { locale: "en" };', shortCircuit: true };
	}
	if (specifier.startsWith('.') && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith('.ts')) {
		return next(specifier + '.ts', context);
	}
	return next(specifier, context);
}`;
register('data:text/javascript,' + encodeURIComponent(hooks));

const require = createRequire(new URL('../../web-client/packages/core/package.json', import.meta.url));
const { JSDOM } = require('jsdom');
const dom = new JSDOM('<!doctype html><html><head></head><body></body></html>');
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.DOMParser = dom.window.DOMParser;
globalThis.Element = dom.window.Element;
globalThis.Node = dom.window.Node;
globalThis.NodeFilter = dom.window.NodeFilter;

const NativeDateTimeFormat = Intl.DateTimeFormat;
Intl.DateTimeFormat = function (locale, options) {
	return new NativeDateTimeFormat(locale === 'default' ? 'en-US' : locale, { ...options, timeZone: 'UTC' });
};

const render = '../../web-client/packages/core/src/mail/render/';
const { sanitizeHtml, scrubCss } = await import(render + 'sanitizeHtml.ts');
const { sanitizeSignatureHtml } = await import(render + 'sanitizeSignature.ts');
const { stripTrackingParams } = await import(render + 'trackingStrip.ts');
const { looksDesigned } = await import(render + 'looksDesigned.ts');
const { plainToHtml } = await import(render + 'plainToHtml.ts');
const { buildSrcDoc } = await import(render + 'renderBody.ts');
const { parseMimeBody } = await import(render + 'parseMime.ts');

const dir = new URL('../../fixtures/render/', import.meta.url);
rmSync(new URL('mime/', dir), { recursive: true, force: true });
mkdirSync(new URL('mime/', dir), { recursive: true });

const write = (name, value) =>
	writeFileSync(new URL(name, dir), JSON.stringify(value, null, '\t') + '\n');

const purifySource = readFileSync(new URL('purify.es.mjs', 'file://' + require.resolve('dompurify')), 'utf8');
const list = (name) => {
	const match = purifySource.match(new RegExp(`const ${name.replace('$', '\\$')} = freeze\\(\\[([\\s\\S]*?)\\]\\)`));
	return JSON.parse(`[${match[1]}]`);
};
const listInConfig = (name) => {
	const match = purifySource.match(new RegExp(`const ${name} = addToSet\\(\\{\\}, \\[([\\s\\S]*?)\\]`));
	return JSON.parse(`[${match[1].replace(/,\s*$/, '')}]`);
};
const propertyNames = (object) => {
	const names = new Set();
	for (let o = object; o; o = Object.getPrototypeOf(o)) {
		for (const name of Object.getOwnPropertyNames(o)) names.add(name);
	}
	return names;
};
const clobber = new Set([
	...propertyNames(dom.window.document),
	...propertyNames(dom.window.document.createElement('form'))
]);
write('dompurify.json', {
	htmlTags: list('html$1'),
	svgTags: list('svg$1'),
	svgFilters: list('svgFilters'),
	svgDisallowed: list('svgDisallowed'),
	mathMlTags: list('mathMl$1'),
	mathMlDisallowed: list('mathMlDisallowed'),
	htmlAttrs: list('html'),
	svgAttrs: list('svg'),
	mathMlAttrs: list('mathMl'),
	xmlAttrs: list('xml'),
	forbidContents: listInConfig('DEFAULT_FORBID_CONTENTS'),
	dataUriTags: listInConfig('DEFAULT_DATA_URI_TAGS'),
	uriSafeAttrs: listInConfig('DEFAULT_URI_SAFE_ATTRIBUTES'),
	clobberNames: [...clobber].sort()
});

const TRACKING = [
	'utm_source', 'utm_medium', 'utm_campaign', 'utm_term', 'utm_content', 'utm_id',
	'utm_source_platform', 'utm_creative_format', 'utm_marketing_tactic', 'fbclid', 'gclid',
	'gbraid', 'wbraid', 'dclid', 'msclkid', 'twclid', 'ttclid', 'igshid', 'wickedid', 'yclid',
	'mc_cid', 'mc_eid', 'mkt_tok', '_hsenc', '_hsmi', 'hsctatracking', '_branch_match_id',
	'vero_id', 'vero_conv', 'ref_src', 'ref_url', 'oly_anon_id', 'oly_enc_id', 'sc_campaign',
	'sc_channel', 'sc_content', 'sc_country', 'sc_geo', 'sc_medium', 'sc_outcome',
	'sc_publisher', 'piwik_campaign', 'piwik_kwd', 'pk_campaign', 'pk_kwd'
];
const trackingInputs = [
	'https://shop.example/item?id=7&utm_source=news&utm_medium=email',
	'https://shop.example/?UTM_Source=a&utm_source=b&keep=1#top',
	'https://shop.example/?fbclid=abc',
	'https://shop.example/?fbclid=abc#frag',
	'http://Shop.EXAMPLE:80/path?q=a%20b&gclid=1',
	'https://shop.example:443/?q=a+b&mc_cid=1&x=%E2%9C%93',
	'https://shop.example/?a&b=&utm_medium=1',
	'https://shop.example/path?keep=1',
	'https://shop.example/path?keep=1&Keep=2',
	'mailto:ada@calloway.test?utm_source=x',
	'HTTPS://shop.example/?utm_term=1',
	'https://sh op.example/?utm_id=1',
	'https://bücher.example/?ttclid=1&q=1',
	'https://xn--bcher-kva.example/?ttclid=1',
	'https://shop.example/?a=1&&utm_content=2&b=3',
	'https://user:pw@shop.example/?msclkid=1&z=1',
	'https://shop.example/?q=%zz&utm_id=1',
	'https://shop.example/?emoji=😀&utm_id=1',
	'https://shop.example/a/../b/./c?utm_id=1&k=v',
	'https://shop.example/?x=a=b&utm_id=1',
	'https://shop.example/?utm_id=1#a b',
	'https://shop.example/?q=~*-._!\'()&utm_id=1',
	'https://shop.example/?q=%2B%26%3D&utm_id=1',
	'https://shop.example?utm_id=1',
	'https://shop.example/p;x?utm_id=1',
	'https://[::1]:8443/?utm_id=1&a=b',
	'https://shop.example/' + '?' + TRACKING.map((k, i) => `${k}=${i}`).join('&') + '&keep=yes',
	'https://shop.example/?' + TRACKING.map((k) => `${k.toUpperCase()}=1`).join('&'),
	'/relative?utm_source=x',
	'https:/shop.example/?utm_source=x',
	'  https://shop.example/?utm_source=x',
	'https://shop.example/?utm_source',
	'https://shop.example/?%75tm_source=1&a=1',
	'https://shop.example/?utm_source=1&a=%C3%A9&b=é'
];
write('tracking.json', trackingInputs.map((input) => ({ input, output: stripTrackingParams(input) })));

const plainInputs = [
	'',
	'hello',
	'a & b < c > d " e \' f',
	'line one\nline two\n\nparagraph two',
	'crlf one\r\ncrlf two\r\n\r\npara\r\n\r\n\r\nthird',
	'see https://example.com/path?a=1&b=2 now',
	'<https://example.com/x>',
	'(https://example.com/paren) and [https://example.com/bracket]',
	'"https://example.com/quoted"',
	'mail ada@calloway.test or bo.b+tag@sub.calloway.test.',
	'not an email: a@b and @x.y and x@y.z1',
	'https://example.com/a@b.com mixed',
	'ftp://example.com and http://example.com',
	'xhttps://example.com and _https://example.com and 1https://example.com',
	'url at end https://example.com',
	'unicode: Привет https://пример.рф/путь мир ada@calloway.test',
	'tab\tseparated\thttps://example.com\tend',
	'\n\nleading blank',
	'trailing blank\n\n',
	'only\n\n\n\n',
	'emoji 😀 https://example.com/😀',
	'nbsp https://example.com/a b',
	'bom﻿https://example.com/a﻿b',
	'nel\u0085https://example.com/a\u0085b',
	'é@calloway.test ada@calloway.tést ada_b@calloway.test',
	'-- \nSignature line\nhttps://calloway.test'
];
write('plain.json', plainInputs.map((input) => ({ input, output: plainToHtml(input) })));

const designedInputs = [
	'<p>plain</p>',
	'<table bgcolor="#f4f4f4"><tr><td>x</td></tr></table>',
	'<table bgcolor="#FFFFFF"><tr><td>x</td></tr></table>',
	'<table bgcolor=white><tr><td>x</td></tr></table>',
	'<td bgcolor="transparent">x</td>',
	'<div style="background:#fff">x</div>',
	'<div style="background-color: rgb(255, 255, 255)">x</div>',
	'<div style="background-color: rgba(255,255,255,0.5)">x</div>',
	'<div style="background-color: rgba(10,10,10,0)">x</div>',
	'<div style="background-color: rgba(10,10,10,0.0)">x</div>',
	'<div style="background: none !important">x</div>',
	'<div style="background: #f00 !important">x</div>',
	'<div style="background-image: url(data:image/png;base64,AA)">x</div>',
	'<div style="BACKGROUND-COLOR: #123456">x</div>',
	'<style>.hero{background:#0E6240}</style><div class="hero">x</div>',
	'<style>body{background:inherit}</style>',
	'<p data-x=" background=1">x</p>',
	'<p title="a background=b">x</p>',
	'<div style="background:;">x</div>',
	'<div style="background: ">x</div>',
	'<div style="background:white;background-color:#eee">x</div>',
	'<font bgcolor="">x</font>',
	'<p>The background: is plain text</p>',
	'<p>bgcolor=red in text</p>',
	'<div style="background-color: #fff">x</div>',
	'<div style="background-color:rgb(255 , 255 , 255 , 1.5)">x</div>'
];
write('designed.json', designedInputs.map((input) => ({ input, output: looksDesigned(input) })));

const cssInputs = [
	'color:red',
	'background:url(cid:hero@calloway.test)',
	'background:url("cid:hero@calloway.test")',
	"background:url('data:image/png;base64,iVBOR')",
	'background:URL( data:image/gif;base64,R0lG )',
	'background:url(https://tracker.example/p.gif)',
	'background:url("https://tracker.example/p.gif") no-repeat',
	'background-image:url(//tracker.example/p.gif), url(cid:a)',
	'list-style:url(http://x.example/b.png) disc',
	'cursor:url(x.cur), auto',
	'@import url(https://evil.example/a.css);\n.a{color:red}',
	'@import "https://evil.example/a.css";.b{color:blue}',
	'@IMPORT url(x.css) screen;',
	'@font-face{font-family:X;src:url(https://fonts.example/x.woff2)}.c{font-family:X}',
	'@font-face {\n  font-family: Y;\n  src: url(data:font/woff2;base64,AA);\n}\np{margin:0}',
	'.a{color:red}@media (max-width:600px){.a{background:url(https://x.example/m.png)}}',
	'td{background:#f4f4f4;padding:0 8px}',
	'filter:url(#shadow)',
	'background:url( https://x.example/a.png  )',
	'body{margin:0}\n.hero{background:url(cid:hero) center/cover}',
	'',
	'   ',
	'a{b:c}'
];
write('css.json', cssInputs.map((input) => ({ input, output: scrubCss(input) })));

const png = Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d]);
const gif = Uint8Array.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0]);
const inline = {
	'hero@calloway.test': { mimeType: 'image/png', data: png },
	'logo': { mimeType: 'image/gif', data: gif }
};
const inlineJson = Object.fromEntries(
	Object.entries(inline).map(([k, v]) => [k, { mimeType: v.mimeType, data: Buffer.from(v.data).toString('base64') }])
);

const newsletter = `<!doctype html><html><head><meta charset="utf-8"><title>Weekly</title>
<style type="text/css">
body{margin:0;background:#f4f4f4}
.wrap{width:600px;margin:0 auto}
@media only screen and (max-width:600px){.wrap{width:100%!important}}
.hero{background:url(cid:hero@calloway.test) center/cover}
</style></head>
<body bgcolor="#f4f4f4" style="margin:0">
<center>
<table role="presentation" class="wrap" width="600" cellpadding="0" cellspacing="0" border="0" align="center" bgcolor="#ffffff">
<tr><td class="hero" height="200"><img src="cid:hero@calloway.test" width="600" alt="Hero"></td></tr>
<tr><td style="padding:24px;font-family:Georgia,serif;font-size:16px;color:#333">
<h1 style="margin:0 0 12px">This week</h1>
<p>Read the <a href="https://news.example/story?id=9&amp;utm_source=newsletter&amp;utm_medium=email">full story</a>.</p>
<p><img src="https://tracker.example/open.gif?u=1" width="1" height="1" alt=""></p>
<table width="100%"><tr><td width="50%" valign="top">Left</td><td width="50%" valign="top" bgcolor="#eeeeee">Right</td></tr></table>
</td></tr>
<tr><td align="center" style="font-size:12px;color:#999">
<a href="https://news.example/unsubscribe?u=1&amp;mc_eid=2">Unsubscribe</a> · <a href="mailto:news@calloway.test">Contact</a>
</td></tr>
</table>
</center>
</body></html>`;

const gmailReply = `<div dir="ltr">Sounds good, see you then.<br clear="all"><div><br></div>-- <br><div dir="ltr" class="gmail_signature" data-smartmail="gmail_signature">Ada Calloway</div></div><br><div class="gmail_quote"><div dir="ltr" class="gmail_attr">On Tue, 3 Nov 2026 at 10:00, Bo Calloway &lt;<a href="mailto:bo@calloway.test">bo@calloway.test</a>&gt; wrote:<br></div><blockquote class="gmail_quote" style="margin:0px 0px 0px 0.8ex;border-left:1px solid rgb(204,204,204);padding-left:1ex"><div dir="ltr">Lunch on Friday?</div></blockquote></div>`;

const outlookReply = `<html xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:w="urn:schemas-microsoft-com:office:word" xmlns:m="http://schemas.microsoft.com/office/2004/12/omml" xmlns="http://www.w3.org/TR/REC-html40"><head><meta http-equiv="Content-Type" content="text/html; charset=utf-8"><meta name="Generator" content="Microsoft Word 15 (filtered medium)"><!--[if !mso]><style>v\\:* {behavior:url(#default#VML);}
o\\:* {behavior:url(#default#VML);}
</style><![endif]--><style><!--
@font-face
	{font-family:"Cambria Math";
	panose-1:2 4 5 3 5 4 6 3 2 4;}
p.MsoNormal, li.MsoNormal, div.MsoNormal
	{margin:0cm;
	font-size:11.0pt;
	font-family:"Calibri",sans-serif;}
a:link, span.MsoHyperlink
	{mso-style-priority:99;
	color:#0563C1;
	text-decoration:underline;}
--></style><!--[if gte mso 9]><xml>
<o:shapedefaults v:ext="edit" spidmax="1026" />
</xml><![endif]--></head><body lang="EN-GB" link="#0563C1" vlink="#954F72" style="word-wrap:break-word"><div class="WordSection1"><p class="MsoNormal">Thanks, approved.<o:p></o:p></p><p class="MsoNormal"><o:p>&nbsp;</o:p></p><div style="border:none;border-top:solid #E1E1E1 1.0pt;padding:3.0pt 0cm 0cm 0cm"><p class="MsoNormal"><b>From:</b> Bo Calloway &lt;bo@calloway.test&gt;<br><b>Sent:</b> 03 November 2026 10:00<br><b>To:</b> Ada Calloway &lt;ada@calloway.test&gt;<br><b>Subject:</b> Budget<o:p></o:p></p></div><p class="MsoNormal"><o:p>&nbsp;</o:p></p><p class="MsoNormal">Please approve.<o:p></o:p></p></div></body></html>`;

const appleReply = `<html><head><meta http-equiv="content-type" content="text/html; charset=utf-8"></head><body style="overflow-wrap: break-word; -webkit-nbsp-mode: space; line-break: after-white-space;">Yes.<br id="lineBreakAtBeginningOfMessage"><div><br><blockquote type="cite"><div>On 3 Nov 2026, at 10:00, Bo Calloway &lt;bo@calloway.test&gt; wrote:</div><br class="Apple-interchange-newline"><div><div>Ready?</div></div></blockquote></div><br></body></html>`;

const sanitizeCases = {
	newsletter,
	'gmail-reply': gmailReply,
	'outlook-reply': outlookReply,
	'apple-reply': appleReply,
	'simple': '<p>Hello <b>world</b> &amp; friends</p>',
	'no-markup': 'plain & simple, no tags here > or there',
	'leading-whitespace': '\n\n  <p>x</p>',
	'rtl': '<div dir="rtl" lang="ar"><p>مرحبا بالعالم</p><p dir="rtl" lang="he">שלום עולם</p></div>',
	'cjk': '<p lang="ja">日本語のテキスト</p><p lang="zh">中文文本</p><p lang="ko">한국어 텍스트</p>',
	'font-center': '<center><font face="Arial" size="2" color="#336699">Old school</font></center>',
	'image-map':
		'<img src="cid:logo" usemap="#m" alt="map"><map name="m"><area shape="rect" coords="0,0,10,10" href="https://calloway.test/a?utm_campaign=x" alt="a"><area shape="rect" coords="10,10,20,20" href="javascript:alert(1)"></map>',
	'cid-variants':
		'<img src="cid:hero@calloway.test"><img src="CID:logo"><img src="cid:<logo>"><img src="cid:missing"><img src=" cid:logo "><img src="cid:Logo">',
	'img-sources':
		'<img src="https://x.example/a.png"><img src="//x.example/a.png"><img src="relative.png"><img src="data:image/png;base64,iVBOR"><img src="data:text/html;base64,PHA+"><img alt="no src">',
	'links':
		'<a href="https://calloway.test/?gclid=1&amp;q=1" target="_self" rel="opener">one</a><a href="mailto:ada@calloway.test">two</a><a href="tel:+15551234">three</a><a href="javascript:void(0)">four</a><a href=" JaVaScRiPt:alert(1)">five</a><a href="vbscript:x">six</a><a href="data:text/html,x">seven</a><a href="#anchor">eight</a><a href="/path">nine</a><a href="ftp://calloway.test">ten</a><a href="calloway.test">eleven</a><a>twelve</a>',
	'attrs':
		'<p class="a" id="intro" title="t" lang="en" dir="ltr" data-track="1" aria-label="x" onclick="x()" style="color:red" align="center" xyz="1">p</p><td width="50%" height="10" valign="top" nowrap colspan="2">c</td>',
	'forbidden-tags':
		'<script>alert(1)</script><iframe src="https://x.example"></iframe><object data="x"></object><embed src="x"><form action="https://x.example"><input name="q" value="v"><button formaction="https://x.example">b</button></form><link rel="stylesheet" href="https://x.example/a.css"><meta http-equiv="refresh" content="0;url=https://x.example"><base href="https://x.example/"><svg><circle r="1"/></svg><math><mi>x</mi></math><p>after</p>',
	'forbidden-attrs':
		'<img src="cid:logo" srcset="https://x.example/a.png 2x"><a href="https://calloway.test" ping="https://x.example/ping">p</a><table background="https://x.example/bg.png"><tr><td>t</td></tr></table>',
	'style-tag':
		'<style>@import url(https://x.example/a.css);@font-face{font-family:X;src:url(https://x.example/x.woff)}.a{background:url(https://x.example/a.png)}.b{background:url(cid:logo)}</style><p class="a">x</p>',
	'style-attr':
		'<div style="background:url(https://x.example/a.png);color:red">x</div><div style="background-image:url(\'cid:logo\')">y</div>',
	'media':
		'<video poster="https://x.example/p.jpg" src="https://x.example/v.mp4" controls><source src="https://x.example/v.webm" type="video/webm"><track src="https://x.example/t.vtt"></video><audio src="https://x.example/a.mp3" controls></audio><input type="image" src="https://x.example/i.png" alt="i"><picture><source srcset="https://x.example/a.webp"><img src="cid:logo"></picture>',
	'comments': '<p>a<!-- hidden --></p><!-- top --><p>b</p><!--[if mso]><p>mso</p><![endif]-->',
	'tables':
		'<table border="1" cellpadding="4" cellspacing="0" width="100%"><caption>Cap</caption><colgroup><col width="30%"><col></colgroup><thead><tr><th scope="col">H</th></tr></thead><tbody><tr><td rowspan="2">1</td></tr></tbody><tfoot><tr><td>f</td></tr></tfoot></table>',
	'lists': '<ul><li>one</li><li>two<ol start="3"><li>three</li></ol></li></ul><dl><dt>t</dt><dd>d</dd></dl>',
	'pre-code': '<pre>  indented\n\tcode &lt;tag&gt;</pre><code>x</code><kbd>k</kbd>',
	'blockquote': '<blockquote type="cite" cite="mid:1@calloway.test"><p>quoted</p></blockquote>',
	'html-body-attrs': '<html lang="en"><body bgcolor="#000000" onload="x()"><p>body</p></body></html>',
	'head-content': '<head><title>T</title><style>p{color:red}</style></head><body><p>x</p></body>',
	'title-in-body': '<p>a</p><title>inside</title><p>b</p>',
	'entities': '<p>&lt;&gt;&amp;&quot;&#39;&nbsp;&copy;&euro;&#x1F600;</p><p title="&quot;q&quot; &amp; &lt;x&gt;">t</p>',
	'unknown-elements': '<custom-tag>c</custom-tag><blink>b</blink><marquee>m</marquee><o:p>o</o:p><xyz>z</xyz>',
	'details': '<details open><summary>S</summary><p>D</p></details>',
	'template': '<template><p>t</p><script>x</script></template><p>after</p>',
	'textarea-select': '<textarea>a &lt;b&gt;</textarea><select><option selected>o</option></select>',
	'id-name-clobber': '<p id="cookie">c</p><p id="body">b</p><a name="location" href="#x">l</a><p id="ok-id">ok</p><img name="getElementById" src="cid:logo">',
	'whitespace-values': '<p title="  padded  " class=" c ">x</p><input value="  keep  ">',
	'nested': '<div><div><div><span><b><i><u>deep</u></i></b></span></div></div></div>',
	'empty-attrs': '<p title="" class="">x</p><a href="">e</a>',
	'uri-attrs': '<p cite="https://calloway.test">c</p><td headers="h1 h2">h</td><img src="cid:logo" longdesc="https://x.example">',
	'xmlns': '<div xmlns="http://www.w3.org/1999/xhtml">x</div>',
	'self-close': '<p title="a/>b">x</p><br/><img src="cid:logo"/>',
	'tracking-case': '<a href="https://calloway.test/?Utm_Source=1&amp;id=2">x</a><area href="https://calloway.test/?fbclid=1" shape="default">'
};

const sanitize = Object.entries(sanitizeCases).map(([name, input]) => ({
	name,
	input,
	strip: sanitizeHtml(input, { inlineImages: inline, stripTracking: true }),
	keep: sanitizeHtml(input, { inlineImages: inline, stripTracking: false })
}));
write('sanitize.json', { inline: inlineJson, cases: sanitize });

const signatureCases = [
	['plain', '<p>Ada Calloway<br>Calloway &amp; Co</p>'],
	['links', '<a href="https://calloway.test">site</a> <a href="mailto:ada@calloway.test">mail</a> <a href="tel:+1">tel</a> <a href="javascript:x">bad</a> <a href=" data:text/html,x">bad2</a>'],
	['marked-image', '<img data-thelemail-sig-image="1" src="data:image/png;base64,AA" alt="logo">'],
	['remote-image', '<img src="https://calloway.test/logo.png" alt="l"><img src="http://calloway.test/l.png">'],
	['data-image', '<img src="data:image/png;base64,AA">'],
	['cid-image', '<img src="cid:logo">'],
	['relative-image', '<img src="logo.png"><img>'],
	['style', '<style>p{color:red}</style><p style="background:url(https://x.example/a.png);color:#333">x</p>'],
	['forbidden', '<script>x</script><iframe></iframe><form><input></form><p onclick="x">p</p>'],
	['table', '<table><tr><td style="padding:4px">a</td><td>b</td></tr></table>'],
	['data-attrs', '<p data-x="1" data-thelemail-sig-image="2">x</p>'],
	['empty', '']
];
const signature = [];
for (const [name, input] of signatureCases) {
	for (const keepRemoteImages of [true, false]) {
		signature.push({ name, input, keepRemoteImages, output: sanitizeSignatureHtml(input, { keepRemoteImages }) });
	}
}
write('signature.json', signature);

const srcdocBodies = [
	['<p>Hello</p>', false],
	['<p>Hello</p>', true],
	['<table bgcolor="#eeeeee"><tr><td>x</td></tr></table>', false],
	['<table bgcolor="#eeeeee"><tr><td>x</td></tr></table>', true],
	['<p class="empty">(no body)</p>', true],
	['', false]
];
write('srcdoc.json', srcdocBodies.map(([body, wrapPlain]) => ({ body, wrapPlain, output: buildSrcDoc(body, wrapPlain) })));

const b64 = (s) => Buffer.from(s).toString('base64').replace(/(.{76})/g, '$1\r\n');
const pngB64 = Buffer.from(png).toString('base64');
const mimeCases = {
	'plain': 'From: Ada <ada@calloway.test>\r\nSubject: Hi\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello there\r\n',
	'html-only': 'Content-Type: text/html; charset=utf-8\r\n\r\n<p>Hi</p>\r\n',
	'no-content-type': 'Subject: bare\r\n\r\nBare body\r\n',
	'alternative':
		'Content-Type: multipart/alternative; boundary="alt"\r\n\r\n--alt\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlain version\r\n--alt\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>HTML version</p>\r\n--alt--\r\n',
	'related':
		'Content-Type: multipart/related; boundary="rel"\r\n\r\n--rel\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p><img src="cid:hero@calloway.test"></p>\r\n--rel\r\nContent-Type: image/png\r\nContent-Transfer-Encoding: base64\r\nContent-ID: <hero@calloway.test>\r\nContent-Disposition: inline; filename="hero.png"\r\n\r\n' + pngB64 + '\r\n--rel--\r\n',
	'mixed-attachment':
		'Content-Type: multipart/mixed; boundary="mix"\r\n\r\n--mix\r\nContent-Type: multipart/alternative; boundary="alt"\r\n\r\n--alt\r\nContent-Type: text/plain\r\n\r\nSee attached\r\n--alt\r\nContent-Type: text/html\r\n\r\n<p>See attached</p>\r\n--alt--\r\n--mix\r\nContent-Type: application/pdf; name="report.pdf"\r\nContent-Disposition: attachment; filename="report.pdf"\r\nContent-Transfer-Encoding: base64\r\n\r\n' + b64('%PDF-1.4 fake') + '\r\n--mix--\r\n',
	'mixed-text-then-html':
		'Content-Type: multipart/mixed; boundary="m2"\r\n\r\n--m2\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nFirst & <plain>\nsecond line\r\n--m2\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Then <b>HTML</b> &amp; more</p><br><ul><li>item</li></ul>\r\n--m2--\r\n',
	'related-without-cid':
		'Content-Type: multipart/related; boundary="r2"\r\n\r\n--r2\r\nContent-Type: text/html\r\n\r\n<p>x</p>\r\n--r2\r\nContent-Type: image/png; name="a.png"\r\nContent-Transfer-Encoding: base64\r\n\r\n' + pngB64 + '\r\n--r2--\r\n',
	'cid-outside-related':
		'Content-Type: multipart/mixed; boundary="m3"\r\n\r\n--m3\r\nContent-Type: text/html\r\n\r\n<p>x</p>\r\n--m3\r\nContent-Type: image/png\r\nContent-ID: <free@calloway.test>\r\nContent-Transfer-Encoding: base64\r\n\r\n' + pngB64 + '\r\n--m3--\r\n',
	'related-nontext-cid':
		'Content-Type: multipart/related; boundary="r3"\r\n\r\n--r3\r\nContent-Type: text/html\r\n\r\n<p>x</p>\r\n--r3\r\nContent-Type: application/octet-stream\r\nContent-ID: <blob>\r\nContent-Transfer-Encoding: base64\r\n\r\n' + b64('blob') + '\r\n--r3--\r\n',
	'quoted-printable':
		'Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n<p>caf=C3=A9 =\r\nsoft break</p>\r\n',
	'latin1':
		'Content-Type: text/plain; charset=iso-8859-1\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\ncaf=E9 na=EFve\r\n',
	'windows-1252':
		'Content-Type: text/plain; charset=windows-1252\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n=93quoted=94 =80 price\r\n',
	'base64-utf8': 'Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n' + b64('Привет, мир 😀') + '\r\n',
	'flowed':
		'Content-Type: text/plain; charset=utf-8; format=flowed\r\n\r\nThis is a long \r\nflowed line.\r\n>quoted \r\n>flowed\r\n-- \r\nsig\r\n',
	'flowed-delsp':
		'Content-Type: text/plain; charset=utf-8; format=flowed; delsp=yes\r\n\r\nJapa \r\nnese\r\n \r\nstuffed\r\n',
	'attachment-text':
		'Content-Type: multipart/mixed; boundary="m4"\r\n\r\n--m4\r\nContent-Type: text/plain\r\n\r\nbody\r\n--m4\r\nContent-Type: text/plain; name="notes.txt"\r\nContent-Disposition: attachment; filename="notes.txt"\r\n\r\nnotes\r\n--m4\r\nContent-Type: text/html\r\nContent-Disposition: attachment; filename="page.html"\r\n\r\n<p>page</p>\r\n--m4--\r\n',
	'attachment-no-filename':
		'Content-Type: multipart/mixed; boundary="m5"\r\n\r\n--m5\r\nContent-Type: text/plain\r\n\r\nbody\r\n--m5\r\nContent-Type: application/octet-stream\r\nContent-Transfer-Encoding: base64\r\n\r\n' + b64('nameless') + '\r\n--m5--\r\n',
	'encoded-filename':
		'Content-Type: multipart/mixed; boundary="m6"\r\n\r\n--m6\r\nContent-Type: text/plain\r\n\r\nbody\r\n--m6\r\nContent-Type: application/pdf; name="=?UTF-8?B?0J7RgtGH0ZHRgi5wZGY=?="\r\nContent-Disposition: attachment\r\nContent-Transfer-Encoding: base64\r\n\r\n' + b64('pdf') + '\r\n--m6--\r\n',
	'calendar':
		'Content-Type: multipart/mixed; boundary="m7"\r\n\r\n--m7\r\nContent-Type: text/plain\r\n\r\nInvite\r\n--m7\r\nContent-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\r\nBEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:1@calloway.test\r\nDTSTART:20261201T100000Z\r\nDTEND:20261201T110000Z\r\nSUMMARY:Lunch\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n--m7--\r\n',
	'two-html-parts':
		'Content-Type: multipart/mixed; boundary="m8"\r\n\r\n--m8\r\nContent-Type: text/html\r\n\r\n<p>one</p>\r\n--m8\r\nContent-Type: text/html\r\n\r\n<p>two</p>\r\n--m8--\r\n',
	'empty-body': 'Content-Type: text/plain\r\n\r\n',
	'whitespace-body': 'Content-Type: text/html\r\n\r\n   \r\n',
	'multipart-digest-free':
		'Content-Type: multipart/mixed; boundary="m9"\r\n\r\n--m9\r\nContent-Type: text/plain\r\n\r\na\r\n--m9\r\nContent-Type: text/plain\r\n\r\nb\r\n--m9--\r\n',
	'apple-structure':
		'Content-Type: multipart/alternative; boundary="A"\r\n\r\n--A\r\nContent-Type: text/plain; charset=us-ascii\r\nContent-Transfer-Encoding: 7bit\r\n\r\nSee photo\r\n--A\r\nContent-Type: multipart/related; type="text/html"; boundary="R"\r\n\r\n--R\r\nContent-Type: text/html; charset=us-ascii\r\n\r\n<html><body>See <img src="cid:p1"></body></html>\r\n--R\r\nContent-Type: image/png; name="p.png"\r\nContent-ID: <p1>\r\nContent-Disposition: inline; filename="p.png"\r\nContent-Transfer-Encoding: base64\r\n\r\n' + pngB64 + '\r\n--R--\r\n\r\n--A--\r\n',
	'forwarded-inline':
		'From: Ada <ada@calloway.test>\r\nContent-Type: multipart/mixed; boundary="F"\r\n\r\n--F\r\nContent-Type: text/plain\r\n\r\nFYI below\r\n--F\r\nContent-Type: message/rfc822\r\n\r\nFrom: "Bo Calloway" <bo@calloway.test>\r\nTo: Ada <ada@calloway.test>, cy@calloway.test\r\nCc: Team: dee@calloway.test, eve@calloway.test;\r\nSubject: =?UTF-8?Q?Caf=C3=A9_plans?=\r\nDate: Tue, 3 Nov 2026 10:00:00 +0100\r\nContent-Type: multipart/alternative; boundary="I"\r\n\r\n--I\r\nContent-Type: text/plain\r\n\r\nInner plain\r\n--I\r\nContent-Type: text/html\r\n\r\n<p>Inner html</p>\r\n--I--\r\n--F--\r\n',
	'forwarded-attachment':
		'Content-Type: multipart/mixed; boundary="G"\r\n\r\n--G\r\nContent-Type: text/plain\r\n\r\nAttached\r\n--G\r\nContent-Type: message/rfc822; name="fwd.eml"\r\nContent-Disposition: attachment; filename="fwd.eml"\r\n\r\nSubject: inner\r\n\r\ninner body\r\n--G--\r\n',
	'delivery-report':
		'Content-Type: multipart/report; report-type=delivery-status; boundary="D"\r\n\r\n--D\r\nContent-Type: text/plain\r\n\r\nDelivery failed\r\n--D\r\nContent-Type: message/delivery-status\r\n\r\nReporting-MTA: dns; mx.calloway.test\r\n--D\r\nContent-Type: message/rfc822\r\n\r\nSubject: original\r\n\r\noriginal body\r\n--D--\r\n',
	'preamble-epilogue':
		'Content-Type: multipart/alternative; boundary="P"\r\n\r\nThis is a preamble.\r\n--P\r\nContent-Type: text/plain\r\n\r\nbody\r\n--P--\r\nepilogue text\r\n',
	'boundary-trailing-space':
		'Content-Type: multipart/mixed; boundary="T"\r\n\r\n--T  \r\nContent-Type: text/plain\r\n\r\nspaced\r\n--T-- \t\r\n',
	'boundary-lookalike':
		'Content-Type: multipart/mixed; boundary="L"\r\n\r\n--L\r\nContent-Type: text/plain\r\n\r\n--Lx not a boundary\r\n--L\r\nContent-Type: text/plain\r\n\r\nsecond\r\n--L--\r\n',
	'lone-cr-and-lf':
		'Content-Type: text/plain\r\n\r\nline\rwith cr\nlf only\r\r\nend\r\n',
	'folded-content-type':
		'Content-Type: text/html;\r\n\tcharset="iso-8859-1"\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n<p>na=EFve</p>\r\n',
	'rfc2231-charset':
		"Content-Type: text/plain; charset*=utf-8''windows-1252\r\n\r\nplain\r\n",
	'unknown-charset': 'Content-Type: text/plain; charset=x-unknown-thing\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nt=E9st\r\n',
	'qp-edge':
		'Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\na=3Db =zz =4 end=\r\n=E2=9C=93 trailing =\r\n',
	'base64-odd':
		'Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8=\r\nIHdvcmxk\r\n!!QQ\r\n',
	'bom-utf8': 'Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n' + b64('﻿bom text') + '\r\n',
	'utf16': 'Content-Type: text/plain; charset=utf-16le\r\nContent-Transfer-Encoding: base64\r\n\r\n' + Buffer.from('wide', 'utf16le').toString('base64') + '\r\n',
	'duplicate-headers':
		'Content-Type: multipart/related; boundary="H"\r\n\r\n--H\r\nContent-Type: text/html\r\n\r\n<p>x</p>\r\n--H\r\nContent-Type: text/plain\r\nContent-Type: image/gif\r\nContent-ID: <first>\r\nContent-ID: <second>\r\nContent-Transfer-Encoding: base64\r\n\r\nR0lGODlh\r\n--H--\r\n',
	'disposition-attachment-html':
		'Content-Type: multipart/mixed; boundary="X"\r\n\r\n--X\r\nContent-Type: text/html\r\nContent-Disposition: attachment\r\n\r\n<p>hidden</p>\r\n--X--\r\n',
	'html-and-plain-groups':
		'Content-Type: multipart/mixed; boundary="Y"\r\n\r\n--Y\r\nContent-Type: multipart/alternative; boundary="Z"\r\n\r\n--Z\r\nContent-Type: text/plain\r\n\r\nfirst plain\r\n--Z\r\nContent-Type: text/html\r\n\r\n<p>first <a href="https://calloway.test">html</a></p><hr><ol><li>one</li></ol>&amp; &copy; &#169; &#xA9; &nbsp; &bogus;\r\n--Z--\r\n--Y\r\nContent-Type: text/html\r\n\r\n<div>second<br>html <!-- c --><script>x</script></div>\r\n--Y\r\nContent-Type: text/plain\r\n\r\nthird plain\r\n--Y--\r\n',
	'header-only': 'Subject: no blank line\r\nContent-Type: text/plain',
	'nested-depth': 'Content-Type: multipart/mixed; boundary="n0"\r\n\r\n' + Array.from({ length: 5 }, (_, i) => `--n${i}\r\nContent-Type: multipart/mixed; boundary="n${i + 1}"\r\n\r\n`).join('') + '--n5\r\nContent-Type: text/plain\r\n\r\ndeep\r\n--n5--\r\n'
};
const mime = {};
for (const [name, raw] of Object.entries(mimeCases)) {
	writeFileSync(new URL(`mime/${name}.eml`, dir), raw, 'utf8');
	let parsed;
	try {
		parsed = await parseMimeBody(raw);
	} catch {
		mime[name] = { error: true };
		continue;
	}
	mime[name] = {
		html: parsed.html ?? null,
		text: parsed.text ?? null,
		inline: Object.fromEntries(
			Object.entries(parsed.inlineImages).map(([k, v]) => [k, { mimeType: v.mimeType, data: Buffer.from(v.data).toString('base64') }])
		)
	};
}
write('mime.json', mime);

console.log(
	`wrote ${trackingInputs.length} tracking, ${plainInputs.length} plain, ${designedInputs.length} designed, ${cssInputs.length} css, ${sanitize.length} sanitize, ${signature.length} signature, ${srcdocBodies.length} srcdoc, ${Object.keys(mime).length} mime`
);
