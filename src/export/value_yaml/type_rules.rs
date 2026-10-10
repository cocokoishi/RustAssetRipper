pub(super) fn is_integer_type_tree_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "bool"
            | "char"
            | "SInt8"
            | "UInt8"
            | "SInt16"
            | "UInt16"
            | "SInt32"
            | "UInt32"
            | "SInt64"
            | "UInt64"
            | "FileSize"
            | "signed char"
            | "unsigned char"
            | "short"
            | "unsigned short"
            | "int"
            | "unsigned int"
            | "long long"
            | "unsigned long long"
            | "Type*"
    )
}

pub(super) fn is_signed_file_id_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "SInt64" | "long long")
}

pub(super) fn is_valid_dictionary_key_type_tree_type(type_name: &str) -> bool {
    type_name == "GUID"
        || matches!(type_name, "string" | "Utf8String" | "float" | "double")
        || is_integer_type_tree_type(type_name)
}

pub(super) fn is_unsigned_byte_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "UInt8" | "byte" | "unsigned char")
}

pub(super) fn is_unsigned_word_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "UInt32" | "uint" | "unsigned int")
}
