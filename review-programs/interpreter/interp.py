# interp.py — the evaluator (tree-walking interpreter) over the node pool.
#
# evaluate() recursively walks the AST by integer index, resolving variables
# against a dict[str, float] environment. Errors are surfaced by raising:
#   NameError          for an unbound variable
#   ZeroDivisionError  for division by zero
#   RuntimeError       for a malformed / unknown node
#
# The environment is a plain dict[str, float]. A `let`-style binding statement
# (`x = <expr>`) evaluates its right-hand side and stores the result back into
# the environment via run_statement(), so later expressions can reference it.

from ast_nodes import NodePool, Node, format_number
from parser import parse_source, ParseResult


# Recursively evaluate the subtree rooted at `index` in `pool`, looking up any
# variables in `env`. Returns the numeric result as a float.
def evaluate(pool: NodePool, index: int, env: dict[str, float]) -> float:
    node: Node = pool.node_at(index)
    kind: str = node.kind

    if kind == "num":
        return node.value

    if kind == "var":
        name: str = node.name
        if name not in env:
            raise NameError("undefined variable: " + name)
        return env[name]

    if kind == "neg":
        return -evaluate(pool, node.left, env)

    if kind == "binop":
        lhs: float = evaluate(pool, node.left, env)
        rhs: float = evaluate(pool, node.right, env)
        return apply_op(node.op, lhs, rhs)

    raise RuntimeError("cannot evaluate node of kind: " + kind)


# Dispatch a binary operator. `match` on the operator string keeps this
# readable; the wildcard arm guards against an operator the parser should
# never have produced.
#
# NOTE: the error message is built into `report` BEFORE the match, not inside
# the `case _:` arm. pyrst's `match` on a str moves the scrutinee, so any later
# reference to `op` (e.g. inside an arm body) fails to borrow-check ("borrow of
# moved value: op"). Precomputing the message sidesteps that.
def apply_op(op: str, lhs: float, rhs: float) -> float:
    report: str = "unknown operator: " + op
    match op:
        case "+":
            return lhs + rhs
        case "-":
            return lhs - rhs
        case "*":
            return lhs * rhs
        case "/":
            if rhs == 0.0:
                raise ZeroDivisionError("division by zero")
            return lhs / rhs
        case _:
            raise RuntimeError(report)


# Parse a single source line and evaluate it against the environment, returning
# the computed value. This is the read-only entry point used by the error-path
# driver: it does NOT attempt to persist a binding, because a dict passed to a
# function is passed by value in pyrst and a write here would not reach the
# caller anyway (the main session uses return-and-reassign for that). The error
# lines exercised through here never bind a variable in any case.
def run_statement(src: str, env: dict[str, float]) -> float:
    result: ParseResult = parse_source(src)
    return evaluate(result.pool, result.root, env)


# Render a result as a clean string (integers without a trailing ".0").
def show(value: float) -> str:
    return format_number(value)
