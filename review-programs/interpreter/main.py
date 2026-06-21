# main.py — driver for the expression interpreter.
#
# Wires the four stages together (tokens -> ast_nodes -> parser -> interp) and
# runs a deterministic scenario: a sequence of source lines is fed through the
# interpreter against a shared variable environment, printing each line's
# parsed form and computed value, then exercising the error paths.

from tokens import Token, tokenize
from parser import parse_source, ParseResult
from ast_nodes import NodePool, render
from interp import evaluate, run_statement, show


# Run one source line, printing its fully-parenthesized parse and its value,
# and RETURN the (possibly updated) environment.
#
# pyrst passes a dict to a function BY VALUE: writing `env[k] = v` inside a
# callee does NOT persist to the caller. The reliable cross-function update
# pattern is return-and-reassign — the callee returns the mutated dict and the
# caller does `env = run_and_print(...)`. That is why this returns the env.
def run_and_print(src: str, env: dict[str, float]) -> dict[str, float]:
    result: ParseResult = parse_source(src)
    # Copy the pool into a LOCAL before using it twice. Passing a struct FIELD
    # (result.pool) to a function moves it (pyrst inserts no clone), so a second
    # function call on the same field fails ("use of moved value"). A local
    # class variable, by contrast, is cloned on each pass, so `pool` can feed
    # both render() and evaluate(). Same for the target string below.
    pool: NodePool = result.pool
    root: int = result.root
    target: str = result.target
    tree: str = render(pool, root)
    value: float = evaluate(pool, root, env)
    if target != "":
        env[target] = value
        print(src + "   ->   " + target + " = " + show(value) + "   [tree " + tree + "]")
    else:
        print(src + "   ->   " + show(value) + "   [tree " + tree + "]")
    return env


def main() -> None:
    print("=== Expression Interpreter ===")

    # Stage 1 demo: show the lexer's token stream for one line.
    print("--- tokens for: 3 + 4 * (2 - 1) ---")
    toks: list[Token] = tokenize("3 + 4 * (2 - 1)")
    for t in toks:
        print("  " + t.describe())

    # Stage 2-4 demo: a running session with persistent variables.
    print("--- evaluation session ---")
    env: dict[str, float] = {}

    # Each call returns the (possibly updated) env, which we reassign so that
    # later statements see earlier bindings (see run_and_print's note).
    env = run_and_print("1 + 2 * 3", env)
    env = run_and_print("(1 + 2) * 3", env)
    env = run_and_print("2 * 3 + 4 * 5", env)
    env = run_and_print("10 - 2 - 3", env)       # left-associative: ((10-2)-3)=5
    env = run_and_print("-5 + 3", env)           # unary minus
    env = run_and_print("2 * -(3 + 1)", env)     # unary minus on a subexpr
    env = run_and_print("3.5 + 1.5", env)        # float literals
    env = run_and_print("7 / 2", env)            # division yields a float

    # Variable bindings persist across statements in `env`.
    env = run_and_print("x = 6 + 4", env)        # x = 10
    env = run_and_print("y = x * 2", env)        # y = 20
    env = run_and_print("x + y", env)            # 30
    env = run_and_print("area = x * y / 2", env) # 100

    # Show the final environment deterministically (sorted keys).
    print("--- final environment ---")
    names: list[str] = sorted(env_keys(env))
    for name in names:
        print("  " + name + " = " + show(env[name]))

    # Error handling: each raises a distinct exception type, caught by name.
    print("--- error handling ---")
    safe_eval("4 / 0", env)                      # ZeroDivisionError
    safe_eval("a + 1", env)                      # NameError (a is unbound)
    safe_eval("2 +", env)                        # SyntaxError (truncated)
    safe_eval("(1 + 2", env)                     # SyntaxError (missing paren)
    safe_eval("1 @ 2", env)                      # ValueError (bad lex char)

    print("=== done ===")


# Collect a dict's keys into a list.
# NOTE: must be `list(env.keys())`, NOT the comprehension `[k for k in env]`.
# pyrst's typeck infers the loop variable of a bare dict comprehension as int
# regardless of the dict's actual key type, so `[k for k in env]` is typed
# list[int] and fails ("expected List(Str), found List(Int)"). Going through
# .keys() types the keys correctly as the dict's key type.
def env_keys(env: dict[str, float]) -> list[str]:
    return list(env.keys())


# Evaluate one line, catching and reporting every error class the pipeline can
# raise. `except E as e` rebinds the raised message as a plain str.
def safe_eval(src: str, env: dict[str, float]) -> None:
    try:
        value: float = run_statement(src, env)
        print("  " + src + "   ->   " + show(value))
    except ZeroDivisionError as e:
        print("  " + src + "   ->   ZeroDivisionError: " + e)
    except NameError as e:
        print("  " + src + "   ->   NameError: " + e)
    except SyntaxError as e:
        print("  " + src + "   ->   SyntaxError: " + e)
    except ValueError as e:
        print("  " + src + "   ->   ValueError: " + e)
    except RuntimeError as e:
        print("  " + src + "   ->   RuntimeError: " + e)
