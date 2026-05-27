# pyrst Grammar

This document defines the formal grammar for the v0 parser. The grammar is based on Python 3's grammar with modifications for Rust code generation and static typing. The parser is indentation-sensitive and uses a recursive-descent approach.

## Lexical Elements

### Tokens

- **Keywords:** `def`, `class`, `if`, `elif`, `else`, `while`, `for`, `in`, `return`, `raise`, `try`, `except`, `finally`, `import`, `from`, `as`, `pass`, `break`, `continue`, `and`, `or`, `not`, `is`, `None`, `True`, `False`, `match`, `case`, `with`, `async`, `await`
- **Operators:** `+`, `-`, `*`, `/`, `//`, `%`, `**`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&`, `|`, `^`, `~`, `<<`, `>>`, `=`, `+=`, `-=`, `*=`, `/=`, `//=`, `%=`, `**=`, `&=`, `|=`, `^=`, `<<=`, `>>=`, `.`, `,`, `:`, `;`, `->`, `...`
- **Delimiters:** `(`, `)`, `[`, `]`, `{`, `}`
- **Literals:** integers, floats, strings (single/double/triple-quoted), identifiers
- **Comments:** `# ...` (line comment, consumed by lexer)
- **Indentation:** INDENT (logical level increase), DEDENT (logical level decrease), NEWLINE

### Indentation Rules

- Physical indentation (spaces/tabs) maps to logical INDENT/DEDENT tokens.
- Indentation increases and decreases are tracked on a stack.
- Blank lines and comment-only lines do not change indentation level.
- Continuation of logical lines (parenthesized expressions, backslash) suppresses INDENT/DEDENT generation.

## Grammar Rules

The grammar is written in EBNF-like notation. `{ rule }` means zero or more; `[ rule ]` means optional.

```
module
    : { import_stmt | definition | simple_stmt } EOF

import_stmt
    : "import" dotted_name [ "as" NAME ]
    | "from" dotted_name "import" NAME [ "as" NAME ] { "," NAME [ "as" NAME ] }

dotted_name
    : NAME { "." NAME }

definition
    : function_def
    | class_def

function_def
    : "def" NAME "(" parameters ")" [ "->" type_expr ] ":" suite

parameters
    : [ parameter { "," parameter } ]

parameter
    : NAME ":" type_expr

class_def
    : "class" NAME [ "(" [ dotted_name ] ")" ] ":" suite

suite
    : simple_stmt
    | NEWLINE INDENT stmt_list DEDENT

stmt_list
    : statement { statement }

statement
    : simple_stmt
    | compound_stmt

simple_stmt
    : small_stmt { ";" small_stmt } [ ";" ] NEWLINE

small_stmt
    : expr_stmt
    | pass_stmt
    | return_stmt
    | raise_stmt
    | break_stmt
    | continue_stmt
    | import_stmt

expr_stmt
    : test_list [ augassign test_list | "=" test_list ]

augassign
    : "+=" | "-=" | "*=" | "/=" | "//=" | "%=" | "**=" | "&=" | "|=" | "^=" | "<<=" | ">>="

test_list
    : test { "," test }

pass_stmt
    : "pass"

return_stmt
    : "return" [ test_list ]

raise_stmt
    : "raise" [ test [ "from" test ] ]

break_stmt
    : "break"

continue_stmt
    : "continue"

compound_stmt
    : if_stmt
    | while_stmt
    | for_stmt
    | try_stmt
    | with_stmt
    | match_stmt

if_stmt
    : "if" test ":" suite { "elif" test ":" suite } [ "else" ":" suite ]

while_stmt
    : "while" test ":" suite [ "else" ":" suite ]

for_stmt
    : "for" exprlist "in" testlist ":" suite [ "else" ":" suite ]

exprlist
    : expr { "," expr }

testlist
    : test { "," test }

try_stmt
    : "try" ":" suite { except_clause } [ "else" ":" suite ] [ "finally" ":" suite ]
    | "try" ":" suite "finally" ":" suite

except_clause
    : "except" [ test [ "as" NAME ] ] ":" suite

with_stmt
    : "with" [ test "as" expr ] ":" suite

match_stmt
    : "match" test ":" NEWLINE INDENT { case_clause } DEDENT

case_clause
    : "case" pattern ":" suite

pattern
    : literal_pattern
    | capture_pattern
    | wildcard_pattern
    | sequence_pattern
    | mapping_pattern
    | class_pattern

literal_pattern
    : NUMBER | STRING | "True" | "False" | "None"

capture_pattern
    : NAME

wildcard_pattern
    : "_"

sequence_pattern
    : "[" [ pattern { "," pattern } ] "]"
    | "(" [ pattern { "," pattern } ] ")"

mapping_pattern
    : "{" [ pattern ":" pattern { "," pattern ":" pattern } ] "}"

class_pattern
    : dotted_name "(" [ pattern { "," pattern } ] ")"

test
    : or_test

or_test
    : and_test { "or" and_test }

and_test
    : not_test { "and" not_test }

not_test
    : "not" not_test
    | comparison

comparison
    : expr { comp_op expr }

comp_op
    : "<" | ">" | "==" | ">=" | "<=" | "!=" | "in" | "not" "in" | "is" | "is" "not"

expr
    : xor_expr { "|" xor_expr }

xor_expr
    : and_expr { "^" and_expr }

and_expr
    : shift_expr { "&" shift_expr }

shift_expr
    : arith_expr { shift_op arith_expr }

shift_op
    : "<<" | ">>"

arith_expr
    : term { add_op term }

add_op
    : "+" | "-"

term
    : factor { mul_op factor }

mul_op
    : "*" | "/" | "//" | "%"

factor
    : ( "+" | "-" | "~" ) factor
    | power

power
    : atom [ "**" factor ]

atom
    : "(" [ test_list ] ")"
    | "[" [ test_list [ comp_for ] ] "]"
    | "{" [ dict_contents [ comp_for ] ] "}"
    | NAME
    | NUMBER
    | STRING { STRING }
    | "None"
    | "True"
    | "False"

dict_contents
    : test ":" test { "," test ":" test }

comp_for
    : "for" exprlist "in" testlist [ comp_for ]

atom_expr
    : [ "await" ] atom { trailer }

trailer
    : "(" [ arglist ] ")"
    | "[" subscript_list "]"
    | "." NAME

subscript_list
    : subscript { "," subscript }

subscript
    : test [ ":" test [ ":" test ] ]
    | ":" test [ ":" test ]

arglist
    : argument { "," argument } [ "," ]

argument
    : ( test [ comp_for ] | test "=" test )

type_expr
    : union_type

union_type
    : intersection_type { "|" intersection_type }

intersection_type
    : primary_type

primary_type
    : atom_type [ "[" type_args "]" ]

atom_type
    : NAME
    | "None"

type_args
    : type_expr { "," type_expr }

list_comp
    : "[" test comp_for "]"

dict_comp
    : "{" test ":" test comp_for "}"

set_comp
    : "{" test comp_for "}"
```

## Precedence and Associativity

| Operator(s) | Precedence | Associativity |
|---|---|---|
| `**` | Highest | Right |
| `+x`, `-x`, `~x` | | Right |
| `*`, `/`, `//`, `%` | | Left |
| `+`, `-` | | Left |
| `<<`, `>>` | | Left |
| `&` | | Left |
| `^` | | Left |
| `\|` | | Left |
| `==`, `!=`, `<`, `>`, `<=`, `>=`, `in`, `is` | | Left |
| `not` | | Right |
| `and` | | Left |
| `or` | Lowest | Left |

## Important Notes

1. **Indentation sensitivity:** The parser uses indentation (INDENT/DEDENT tokens) to delimit blocks, like Python. Parenthesized, bracketed, and braced expressions suppress indentation tracking.

2. **Newline handling:** Most statements end with NEWLINE. However, a logical line can continue across multiple physical lines if it is inside parentheses, brackets, or braces, or if it ends with a backslash.

3. **Type annotations:** Function parameters and return types require type annotations (no implicit `Any`). Class attributes require type annotations. Local variables may have inferred types if the context is clear.

4. **Comments:** Trailing comments (after a statement on the same line) are allowed. Comments consume characters from `#` to end-of-line and do not appear in the token stream.

5. **Reserved for future expansion:**
   - `async`, `await` (deferred to v1.0)
   - `match`, `case` (deferred to v0.2)
   - Pattern matching syntax is recognized but semantics are deferred

6. **Operator precedence follows Python** but with explicit rules for clarity. The grammar avoids left recursion by using loops and alternation.

7. **Atoms and trailers:** An atom (name, literal, parenthesized expression, list/dict/set) can be followed by zero or more trailers (function calls, subscripts, attribute access). This handles method calls, indexing, etc.

## CST to AST Conversion

The parser emits a Concrete Syntax Tree (CST) that preserves all tokens, whitespace, and comments. The CST is then converted to an Abstract Syntax Tree (AST) that discards trivia and organizes semantic elements. The conversion happens after parsing and before type checking.

Key CST preservation goals:
- Token spans (line, column, length) for source mapping and diagnostics.
- Comments (associated with nearby nodes) for formatter and documentation tools.
- Parenthesization (e.g., `(a + b) * c` vs. `a + b * c`) for pretty-printing.

## Character Encoding

- **Source files:** UTF-8 (or ASCII subset).
- **Identifiers:** Alphanumeric + underscore; must start with letter or underscore.
- **Strings:** Single-quoted `'...'`, double-quoted `"..."`, or triple-quoted `'''...'''` and `"""..."""`. Escape sequences: `\n`, `\t`, `\r`, `\\`, `\'`, `\"`. Unicode escapes: `\uXXXX`, `\UXXXXXXXX` (deferred).
