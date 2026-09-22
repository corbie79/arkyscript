(comment) @comment
(hash_bang_line) @comment
(string) @string
(escape_sequence) @string.escape
(number) @number
(break_statement) @keyword
[(true) (false)] @boolean
(nil) @constant.builtin
(identifier) @variable
(primitive_type) @type.builtin
(named_type) @type
(class_declaration name: (identifier) @type)
(trait_declaration name: (identifier) @type)
(class_field name: (identifier) @property)
(trait_requirement name: (identifier) @function.method)
(method_declaration name: (identifier) @function.method)
(parameter name: (identifier) @variable.parameter)
(type_field name: (identifier) @property)
(field name: (identifier) @property)
(dot_index_expression field: (identifier) @property)
(optional_dot_index_expression field: (identifier) @property)
(method_index_expression method: (identifier) @function.method)
(optional_method_index_expression method: (identifier) @function.method)
(function_declaration name: (identifier) @function)
(function_call name: (identifier) @function)
(optional_function_call name: (identifier) @function)
(function_declaration name: (dot_index_expression field: (identifier) @function))
["local" "function" "return" "end" "if" "then" "else" "elseif"
 "do" "while" "repeat" "until" "for" "in" "export"
 "import" "from" "declare" "try" "except" "class" "trait" "is"] @keyword
["and" "or" "not"] @keyword.operator
["+" "-" "*" "/" "%" "^" "#" "==" "~=" "<=" ">=" "<" ">" "=" ".." "?" "..."] @operator
["?." "?[" "?(" "?:"] @operator
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
[":" "," ";" "."] @punctuation.delimiter

(import_statement name: (identifier) @variable)
(try_statement error: (identifier) @variable)
(declare_statement name: (identifier) @property)
