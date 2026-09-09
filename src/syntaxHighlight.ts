/**
 * 轻量语法高亮（零依赖）。
 *
 * 设计目标：
 * - 按代码关键字着色，参考 IntelliJ IDEA 的默认配色（Light / Darcula）。
 * - 颜色走 CSS 变量（--syntax-*），随 SnapGit 的 light / dark 主题自动切换。
 * - 纯文本逐行 tokenize，保证与 diff 面板 / 编辑窗口的等宽对齐完全一致
 *   （overlay 文本与 textarea 逐字符对齐，仅包裹颜色，不改变任何字符）。
 * - 多行构造（块注释、模板字符串跨行）按「单行内完整闭合」处理；未闭合则本行剩余
 *   按该语义着色，下一行复位——这是逐行高亮的已知取舍，足够覆盖绝大多数代码场景。
 */

export type TokenType =
  | 'plain'
  | 'keyword'
  | 'string'
  | 'comment'
  | 'number'
  | 'function'
  | 'type'
  | 'constant'

export interface Token {
  text: string
  type: TokenType
}

export type LangId =
  | 'plain'
  | 'js'
  | 'python'
  | 'rust'
  | 'go'
  | 'java'
  | 'cpp'
  | 'shell'
  | 'yaml'
  | 'toml'
  | 'sql'
  | 'json'
  | 'css'
  | 'html'
  | 'xml'
  | 'markdown'

interface LangDef {
  lineComments: string[]
  blockComment?: [string, string]
  strings: string[]
  keywords: string[]
}

const JS_KW = [
  'const', 'let', 'var', 'function', 'return', 'if', 'else', 'for', 'while', 'do',
  'switch', 'case', 'break', 'continue', 'new', 'class', 'extends', 'super', 'this',
  'typeof', 'instanceof', 'in', 'of', 'void', 'delete', 'yield', 'await', 'async',
  'import', 'export', 'from', 'as', 'default', 'try', 'catch', 'finally', 'throw',
  'null', 'true', 'false', 'undefined', 'enum', 'interface', 'type', 'implements',
  'public', 'private', 'protected', 'readonly', 'static', 'get', 'set', 'namespace',
  'declare', 'abstract',
]

const PY_KW = [
  'def', 'class', 'return', 'if', 'elif', 'else', 'for', 'while', 'break', 'continue',
  'pass', 'import', 'from', 'as', 'except', 'try', 'finally', 'raise', 'with', 'yield',
  'lambda', 'global', 'nonlocal', 'assert', 'del', 'in', 'is', 'not', 'and', 'or',
  'None', 'True', 'False', 'async', 'await',
]

const RUST_KW = [
  'fn', 'let', 'mut', 'const', 'static', 'struct', 'enum', 'trait', 'impl', 'pub',
  'use', 'mod', 'return', 'if', 'else', 'for', 'while', 'loop', 'match', 'break',
  'continue', 'where', 'async', 'await', 'move', 'ref', 'type', 'self', 'Self', 'as',
  'dyn', 'unsafe', 'extern', 'crate', 'super', 'true', 'false', 'Some', 'Ok', 'Err',
  'None', 'in',
]

const GO_KW = [
  'func', 'var', 'const', 'type', 'struct', 'interface', 'map', 'chan', 'go', 'defer',
  'return', 'if', 'else', 'for', 'range', 'switch', 'case', 'default', 'break',
  'continue', 'package', 'import', 'select', 'fallthrough', 'nil', 'true', 'false',
]

const JAVA_KW = [
  'class', 'interface', 'enum', 'public', 'private', 'protected', 'static', 'final',
  'void', 'int', 'long', 'double', 'float', 'boolean', 'char', 'byte', 'short',
  'String', 'return', 'if', 'else', 'for', 'while', 'do', 'switch', 'case', 'break',
  'continue', 'new', 'import', 'package', 'extends', 'implements', 'throws', 'throw',
  'try', 'catch', 'finally', 'this', 'super', 'abstract', 'synchronized', 'volatile',
  'transient', 'instanceof', 'true', 'false', 'null', 'var', 'record', 'sealed',
  'permits', 'yield', 'strictfp',
]

const CPP_KW = [
  'int', 'char', 'float', 'double', 'long', 'short', 'unsigned', 'signed', 'void',
  'bool', 'struct', 'class', 'enum', 'union', 'const', 'static', 'public', 'private',
  'protected', 'return', 'if', 'else', 'for', 'while', 'do', 'switch', 'case',
  'break', 'continue', 'new', 'delete', 'namespace', 'using', 'template', 'typename',
  'typedef', 'this', 'sizeof', 'true', 'false', 'nullptr', 'auto', 'virtual',
  'override', 'final', 'extern', 'inline',
]

const SH_KW = [
  'if', 'then', 'else', 'elif', 'fi', 'for', 'while', 'do', 'done', 'case', 'esac',
  'in', 'function', 'return', 'export', 'local', 'echo', 'cd', 'exit', 'source',
]

const SQL_KW = [
  'SELECT', 'FROM', 'WHERE', 'INSERT', 'INTO', 'VALUES', 'UPDATE', 'SET', 'DELETE',
  'CREATE', 'TABLE', 'DROP', 'ALTER', 'TRUNCATE', 'INDEX', 'VIEW', 'AND', 'OR', 'NOT',
  'NULL', 'PRIMARY', 'FOREIGN', 'KEY', 'REFERENCES', 'UNIQUE', 'JOIN', 'LEFT', 'RIGHT',
  'INNER', 'OUTER', 'FULL', 'ON', 'BY', 'GROUP', 'ORDER', 'HAVING', 'LIMIT', 'OFFSET',
  'AS', 'DISTINCT', 'CASE', 'WHEN', 'THEN', 'ELSE', 'END', 'BEGIN', 'COMMIT', 'ROLLBACK',
  'TRANSACTION', 'REPLACE', 'DEFAULT', 'ASC', 'DESC', 'LIKE', 'BETWEEN', 'IS', 'EXISTS',
  'ALL', 'ANY', 'UNION', 'GRANT', 'REVOKE', 'USE', 'DATABASE', 'SCHEMA',
]

const LANG_DEFS: Record<LangId, LangDef> = {
  plain: { lineComments: [], strings: [], keywords: [] },
  js: { lineComments: ['//'], blockComment: ['/*', '*/'], strings: ['double', 'single', 'backtick'], keywords: JS_KW },
  python: { lineComments: ['#'], strings: ['double', 'single', 'triple-double', 'triple-single'], keywords: PY_KW },
  rust: { lineComments: ['//'], blockComment: ['/*', '*/'], strings: ['double', 'single'], keywords: RUST_KW },
  go: { lineComments: ['//'], blockComment: ['/*', '*/'], strings: ['double', 'single', 'backtick'], keywords: GO_KW },
  java: { lineComments: ['//'], blockComment: ['/*', '*/'], strings: ['double', 'single'], keywords: JAVA_KW },
  cpp: { lineComments: ['//'], blockComment: ['/*', '*/'], strings: ['double', 'single'], keywords: CPP_KW },
  shell: { lineComments: ['#'], strings: ['double', 'single'], keywords: SH_KW },
  yaml: { lineComments: ['#'], strings: ['double', 'single'], keywords: [] },
  toml: { lineComments: ['#'], strings: ['double', 'single'], keywords: [] },
  sql: { lineComments: ['--'], blockComment: ['/*', '*/'], strings: ['single', 'double'], keywords: SQL_KW },
  json: { lineComments: [], strings: ['double'], keywords: ['true', 'false', 'null'] },
  css: { lineComments: [], blockComment: ['/*', '*/'], strings: ['double', 'single'], keywords: [] },
  html: { lineComments: [], blockComment: ['<!--', '-->'], strings: ['double', 'single'], keywords: [] },
  xml: { lineComments: [], blockComment: ['<!--', '-->'], strings: ['double', 'single'], keywords: [] },
  markdown: { lineComments: [], strings: ['double', 'single'], keywords: [] },
}

function esc(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&')
}

interface Pattern {
  re: RegExp
  type: TokenType
}

interface Compiled {
  patterns: Pattern[]
}

// 同一语言的编译产物（正则实例）只构建一次；tokenize 时按 pos 复位 lastIndex 复用，避免重复编译。
const compiledCache = new Map<LangId, Compiled>()

function buildCompiled(def: LangDef): Compiled {
  const patterns: Pattern[] = []

  // 优先级：块注释 > 行注释 > 字符串 > 数字 > 关键字 > 常量 > 函数 > 类型
  // （同一起点取数组靠前者，故按此顺序入栈）
  if (def.blockComment) {
    const [o, c] = def.blockComment
    patterns.push({ re: new RegExp(esc(o) + '[\\s\\S]*?' + esc(c), 'g'), type: 'comment' })
  }
  for (const lc of def.lineComments) {
    patterns.push({ re: new RegExp(esc(lc) + '[^\\n]*', 'g'), type: 'comment' })
  }

  const sp: string[] = []
  for (const s of def.strings) {
    if (s === 'double') sp.push('"(?:\\\\.|[^"\\\\])*"')
    else if (s === 'single') sp.push("'(?:\\\\.|[^'\\\\])*'")
    else if (s === 'backtick') sp.push('`(?:\\\\.|[^`\\\\])*`')
    else if (s === 'triple-double') sp.push('"""(?:\\\\.|[^\\\\])*?"""')
    else if (s === 'triple-single') sp.push("'''(?:\\\\.|[^\\\\])*?'''")
  }
  if (sp.length) {
    patterns.push({ re: new RegExp('(?:' + sp.join('|') + ')', 'g'), type: 'string' })
  }

  patterns.push({
    re: /\b(?:0x[\da-fA-F]+|\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?)\b/g,
    type: 'number',
  })

  if (def.keywords.length) {
    patterns.push({
      re: new RegExp('\\b(?:' + def.keywords.map(esc).join('|') + ')\\b', 'g'),
      type: 'keyword',
    })
  }

  // 全大写常量：TRUE / NULL / HTTPS ...
  patterns.push({ re: /\b[A-Z][A-Z0-9_]{2,}\b/g, type: 'constant' })
  // 函数调用：标识符后紧跟 ( （含空白）
  patterns.push({ re: /[A-Za-z_$][\w$]*(?=\s*\()/g, type: 'function' })
  // 类型：首字母大写标识符（User / String / Int ...）
  patterns.push({ re: /\b[A-Z][A-Za-z0-9_$]*\b/g, type: 'type' })

  return { patterns }
}

function getCompiled(lang: LangId): Compiled {
  let c = compiledCache.get(lang)
  if (c) return c
  c = buildCompiled(LANG_DEFS[lang])
  compiledCache.set(lang, c)
  return c
}

/**
 * 对单行文本做语法 tokenize，返回按原顺序拼接等于原文的 token 数组。
 * 零长度匹配会被跳过（避免死循环）；任何未命中规则的字符归为 plain。
 */
function tokenize(text: string, lang: LangId): Token[] {
  if (lang === 'plain' || !text) return [{ text, type: 'plain' }]
  const comp = getCompiled(lang)
  const tokens: Token[] = []
  let pos = 0
  const n = text.length
  let guard = 0

  while (pos < n) {
    if (++guard > 20000) {
      tokens.push({ text: text.slice(pos), type: 'plain' })
      break
    }

    let bestIdx = -1
    let bestLen = 0
    let bestType: TokenType = 'plain'

    for (const p of comp.patterns) {
      p.re.lastIndex = pos
      const m = p.re.exec(text)
      if (!m) continue
      const len = m[0].length
      if (len === 0) {
        p.re.lastIndex = pos + 1
        continue
      }
      // 取最早命中的；同起点取数组中靠前的（优先级更高）
      if (bestIdx === -1 || m.index < bestIdx) {
        bestIdx = m.index
        bestLen = len
        bestType = p.type
      }
    }

    if (bestIdx === -1) {
      tokens.push({ text: text.slice(pos), type: 'plain' })
      break
    }
    if (bestIdx > pos) {
      tokens.push({ text: text.slice(pos, bestIdx), type: 'plain' })
    }
    tokens.push({ text: text.slice(bestIdx, bestIdx + bestLen), type: bestType })
    pos = bestIdx + bestLen
  }

  return tokens
}

// 行级缓存：同一语言下相同文本行（空白行、`}`、常见行）命中率极高，显著降低大文件开销。
const lineCache = new Map<string, Token[]>()

function cachedTokenize(text: string, lang: LangId): Token[] {
  const key = lang + ' ' + text
  const hit = lineCache.get(key)
  if (hit) return hit
  const toks = tokenize(text, lang)
  if (lineCache.size > 20000) lineCache.clear()
  lineCache.set(key, toks)
  return toks
}

/** 对外入口：对单行文本做语法高亮，返回 token 数组（拼接等于原文）。 */
export function tokenizeLine(text: string, lang: LangId): Token[] {
  return cachedTokenize(text, lang)
}

/** 根据文件路径推断语言（按扩展名）。未知扩展名回落为 plain（不高亮）。 */
export function fileLang(path: string): LangId {
  const ext = (path.split('.').pop() ?? '').toLowerCase()
  switch (ext) {
    case 'ts':
    case 'tsx':
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs':
    case 'vue':
      return 'js'
    case 'py':
      return 'python'
    case 'rs':
      return 'rust'
    case 'go':
      return 'go'
    case 'java':
    case 'kt':
    case 'kts':
      return 'java'
    case 'c':
    case 'h':
    case 'cpp':
    case 'cc':
    case 'cxx':
    case 'hpp':
    case 'hh':
      return 'cpp'
    case 'sh':
    case 'bash':
    case 'zsh':
      return 'shell'
    case 'yaml':
    case 'yml':
      return 'yaml'
    case 'toml':
      return 'toml'
    case 'sql':
      return 'sql'
    case 'json':
    case 'jsonc':
      return 'json'
    case 'css':
    case 'scss':
    case 'less':
      return 'css'
    case 'html':
    case 'htm':
      return 'html'
    case 'xml':
    case 'svg':
      return 'xml'
    case 'md':
    case 'markdown':
      return 'markdown'
    default:
      return 'plain'
  }
}
