# ast_nodes.py — the abstract syntax tree, stored as a flat node pool.
#
# A self-referential class field (`left: Node` inside `class Node`) does NOT
# compile in pyrst: the generated Rust struct is recursive with no Box/Rc
# indirection and rustc rejects it ("recursive type has infinite size").
#
# The portable workaround is a NODE POOL: every Node lives in one shared
# list[Node], and a node refers to its children by their integer index in
# that pool (not by holding another Node). A child index of -1 means "none".
# This is a classic arena / flat-tree representation and it compiles cleanly.
#
# Node kinds (string tags, since pyrst has no enums):
#   "num"    numeric literal; payload in `value`
#   "var"    variable reference; payload in `name`
#   "neg"    unary negation; operand index in `left`
#   "binop"  binary operation; `op` holds + - * /, operands in `left`/`right`


class Node:
    kind: str
    value: float
    name: str
    op: str
    left: int
    right: int

    def __init__(self, kind: str) -> None:
        self.kind = kind
        self.value = 0.0
        self.name = ""
        self.op = ""
        self.left = -1
        self.right = -1


# The pool owns the nodes; constructors push a node and return its index so
# callers can wire parents to children purely by integer handles.
class NodePool:
    nodes: list[Node]

    def __init__(self) -> None:
        self.nodes = []

    def add_num(self, value: float) -> int:
        node: Node = Node("num")
        node.value = value
        self.nodes.append(node)
        return len(self.nodes) - 1

    def add_var(self, name: str) -> int:
        node: Node = Node("var")
        node.name = name
        self.nodes.append(node)
        return len(self.nodes) - 1

    def add_neg(self, operand: int) -> int:
        node: Node = Node("neg")
        node.left = operand
        self.nodes.append(node)
        return len(self.nodes) - 1

    def add_binop(self, op: str, left: int, right: int) -> int:
        node: Node = Node("binop")
        node.op = op
        node.left = left
        node.right = right
        self.nodes.append(node)
        return len(self.nodes) - 1

    # NOTE: this method is intentionally NOT named `get`. pyrst's codegen
    # special-cases any `.get(...)` call as a dict lookup
    # (`.get(&k).cloned().unwrap_or(default)`) based on the method name alone,
    # with no check of the receiver type — so a user-defined `get` method is
    # silently hijacked and produces invalid Rust. `node_at` sidesteps that.
    def node_at(self, index: int) -> Node:
        return self.nodes[index]

    def size(self) -> int:
        return len(self.nodes)


# Render a node (and, recursively, its subtree) as a fully-parenthesized
# string. Useful for showing how precedence was resolved. Recursion here
# walks the pool by index, which is the whole point of the flat design.
def render(pool: NodePool, index: int) -> str:
    node: Node = pool.node_at(index)
    kind: str = node.kind

    if kind == "num":
        return format_number(node.value)
    if kind == "var":
        return node.name
    if kind == "neg":
        return "(-" + render(pool, node.left) + ")"
    if kind == "binop":
        lhs: str = render(pool, node.left)
        rhs: str = render(pool, node.right)
        return "(" + lhs + " " + node.op + " " + rhs + ")"
    return "?"


# Print whole numbers without a trailing ".0" so output reads naturally,
# but keep real fractions. pyrst prints floats with a ".0" by default, so we
# special-case the integral case by hand.
def format_number(x: float) -> str:
    truncated: int = int(x)
    if float(truncated) == x:
        return str(truncated)
    return str(x)
