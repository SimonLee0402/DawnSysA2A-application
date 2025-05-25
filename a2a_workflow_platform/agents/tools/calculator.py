import ast
import operator
from typing import Dict, Any, Union

from .base import BaseTool

# Define a type for supported operations
OP_MAP = {
    ast.Add: operator.add,
    ast.Sub: operator.sub,
    ast.Mult: operator.mul,
    ast.Div: operator.truediv,
    ast.Pow: operator.pow,
    ast.BitXor: operator.xor,
    ast.USub: operator.neg
}

class CalculatorTool(BaseTool):
    # Define metadata as class attributes
    name = "calculator"
    name_zh = "计算器"
    description = "A simple calculator tool that can perform basic arithmetic operations like addition, subtraction, multiplication, and division. Supports expressions like '2+2' or '10-5*2'."
    description_zh = "一个可以执行基本算术运算（如加、减、乘、除）的简单工具。支持类似 '2+2' 或 '10-5*2' 的表达式。"
    parameters = {
        "type": "object",
        "properties": {
            "expression": {
                "type": "string",
                "description": "The arithmetic expression to evaluate. Example: '2+2' or '10/2-3'"
            }
        },
        "required": ["expression"]
    }

    def _eval_expr(self, node: Union[ast.Expression, ast.Num, ast.Name, ast.BinOp, ast.UnaryOp, ast.Call]) -> Union[int, float]:
        if isinstance(node, ast.Num):
            return node.n
        elif isinstance(node, ast.BinOp):
            return OP_MAP[type(node.op)](self._eval_expr(node.left), self._eval_expr(node.right))
        elif isinstance(node, ast.UnaryOp): # Support for unary operations like negation
            return OP_MAP[type(node.op)](self._eval_expr(node.operand))
        else:
            # For simplicity, raising error for unsupported nodes. 
            # Could be extended for variables, functions if needed, but that increases security risks.
            raise TypeError(f"Unsupported operation or node type: {type(node).__name__}")

    def execute(self, params: Dict[str, Any]) -> Dict[str, Any]:
        expression = params.get("expression")
        if not expression:
            return {"error": "Expression not provided."}

        try:
            # Parse the expression string into an AST node
            # ast.parse returns a Module, we need the first (and only) expression in the body
            parsed_expr = ast.parse(expression, mode='eval').body
            result = self._eval_expr(parsed_expr)
            return {"result": result}
        except (SyntaxError, TypeError, KeyError, ZeroDivisionError) as e:
            # KeyError for unsupported operations in OP_MAP
            # ZeroDivisionError for division by zero
            return {"error": f"Error evaluating expression: {str(e)}"}
        except Exception as e:
            # Catch any other unexpected errors during evaluation
            return {"error": f"An unexpected error occurred: {str(e)}"}

# Example usage (for testing):
# if __name__ == '__main__':
#     calc = CalculatorTool()
#     print(f"Schema: {calc.get_schema()}")
#     test_expressions = ["2+3*4", "(100-20)/2", "sqrt(16)", "pow(2,3)", "sin(0.5)", "10/0", "invalid_func(5)"] # Complex expressions will fail
#     simple_expressions = ["2", "3.14", "10 + 5", "20 - 3", "7 * 8", "100 / 4", "10 / 0", "5 x 6"]
#     invalid_expressions = ["sqrt(16)", "(2+3)*4", "1 2 3 4", "2+", "test"]
#     for expr in simple_expressions:
#         print(f"Expression: '{expr}' -> Result: {calc.execute({'expression': expr})})")
#     for expr in invalid_expressions:
#         print(f"Expression: '{expr}' -> Result: {calc.execute({'expression': expr})})")
#     print(f"Expression: {{'expression': 2+3}} -> Result: {calc.execute({'expression': 2+3})})") # Invalid param type 