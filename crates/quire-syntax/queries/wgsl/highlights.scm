; WGSL highlights for tree-sitter-wgsl-bevy 0.1.4. quire query, MIT OR Apache-2.0.
; The grammar has no highlight query of its own. Later patterns win over earlier ones.

(identifier) @variable

(type_declaration
  (identifier) @type)

(struct_declaration
  name: (identifier) @type)

(type_alias_declaration
  (identifier) @type)

(struct_member
  (variable_identifier_declaration
    name: (identifier) @property))

(parameter
  (variable_identifier_declaration
    name: (identifier) @variable.parameter))

(composite_value_decomposition_expression
  accessor: (identifier) @property)

(function_declaration
  name: (identifier) @function)

(type_constructor_or_function_call_expression
  (type_declaration
    (identifier) @function))

((type_constructor_or_function_call_expression
  (type_declaration
    (identifier) @function.builtin))
 (#match? @function.builtin "^(abs|acos|acosh|all|any|arrayLength|asin|asinh|atan|atan2|atanh|ceil|clamp|cos|cosh|countLeadingZeros|countOneBits|countTrailingZeros|cross|degrees|determinant|distance|dot|dpdx|dpdxCoarse|dpdxFine|dpdy|dpdyCoarse|dpdyFine|exp|exp2|extractBits|faceForward|firstLeadingBit|firstTrailingBit|floor|fma|fract|frexp|fwidth|fwidthCoarse|fwidthFine|insertBits|inverseSqrt|ldexp|length|log|log2|max|min|mix|modf|normalize|pow|quantizeToF16|radians|reflect|refract|reverseBits|round|saturate|select|sign|sin|sinh|smoothstep|sqrt|step|storageBarrier|tan|tanh|textureDimensions|textureGather|textureGatherCompare|textureLoad|textureNumLayers|textureNumLevels|textureNumSamples|textureSample|textureSampleBias|textureSampleCompare|textureSampleCompareLevel|textureSampleGrad|textureSampleLevel|textureStore|transpose|trunc|unpack2x16float|unpack2x16snorm|unpack2x16unorm|unpack4x8snorm|unpack4x8unorm|pack2x16float|pack2x16snorm|pack2x16unorm|pack4x8snorm|pack4x8unorm|workgroupBarrier|workgroupUniformLoad)$"))

[
  "bool" "f16" "f32" "i32" "u32"
  "vec2" "vec3" "vec4"
  "mat2x2" "mat2x3" "mat2x4" "mat3x2" "mat3x3" "mat3x4" "mat4x2" "mat4x3" "mat4x4"
  "array" "ptr" "sampler" "sampler_comparison"
  "texture_1d" "texture_2d" "texture_2d_array" "texture_3d" "texture_cube" "texture_cube_array"
  "texture_depth_2d" "texture_depth_2d_array" "texture_depth_cube" "texture_depth_cube_array"
  "texture_depth_multisampled_2d" "texture_multisampled_2d" "texture_storage_1d"
  "texture_storage_2d" "texture_storage_2d_array" "texture_storage_3d"
] @type.builtin

(attribute
  (identifier) @attribute)

(address_space) @keyword
(access_mode) @keyword
(texel_format) @constant

[
  (int_literal)
  (float_literal)
] @number

(bool_literal) @boolean

(line_comment) @comment
(block_comment) @comment

[
  "fn"
  "struct"
  "var"
  "let"
  "override"
  "return"
  "if"
  "else"
  "for"
  "while"
  "loop"
  "switch"
  "case"
  "default"
  "break"
  "continue"
  "continuing"
  "discard"
  "fallthrough"
  "bitcast"
  "as"
  "type"
  "function"
  "enable"
] @keyword

[
  "="
  "+"
  "-"
  "*"
  "/"
  "%"
  "&"
  "|"
  "^"
  "~"
  "!"
  "<"
  ">"
  "<="
  ">="
  "=="
  "!="
  "&&"
  "||"
  "<<"
  ">>"
  "->"
  "++"
  "--"
] @operator

(compound_assignment_operator) @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ";"
  ":"
  "."
] @punctuation.delimiter
