use std::io::Write;

use crate::language::SupportedLanguage;
use crate::parser::ParsedData;
use crate::rust_types::{RustItem, RustTypeFormatError, SpecialRustType};
use crate::{
    language::Language,
    rust_types::{RustEnum, RustField, RustStruct, RustTypeAlias},
    topsort::topsort,
};
use std::collections::{HashMap, HashSet};

/// All information needed to generate Go type-code
#[derive(Default)]
pub struct Python {
    /// Conversions from Rust type names to Go type names.
    pub type_mappings: HashMap<String, String>,
}

impl Language for Python {
    fn generate_types(&mut self, w: &mut dyn Write, data: &ParsedData) -> std::io::Result<()> {
        // Generate a list of all types that either are a struct or are aliased to a struct.
        // This is used to determine whether a type should be defined as a pointer or not.
        let mut types_mapping_to_struct = HashSet::new();
        for s in &data.structs {
            types_mapping_to_struct.insert(s.id.original.as_str());
        }
        for alias in &data.aliases {
            if types_mapping_to_struct.contains(&alias.r#type.id()) {
                types_mapping_to_struct.insert(alias.id.original.as_str());
            }
        }

        self.begin_file(w)?;

        let mut items: Vec<RustItem> = vec![];

        for a in &data.aliases {
            items.push(RustItem::Alias(a.clone()))
        }

        for s in &data.structs {
            items.push(RustItem::Struct(s.clone()))
        }

        for e in &data.enums {
            items.push(RustItem::Enum(e.clone()))
        }

        let sorted = topsort(items.iter().collect());

        for &thing in &sorted {
            match thing {
                RustItem::Enum(e) => self.write_enum(w, e, &types_mapping_to_struct)?,
                RustItem::Struct(s) => self.write_struct(w, s)?,
                RustItem::Alias(a) => self.write_type_alias(w, a)?,
            }
        }

        self.end_file(w)?;

        Ok(())
    }

    fn type_map(&mut self) -> &HashMap<String, String> {
        &self.type_mappings
    }

    fn format_special_type(
        &mut self,
        special_ty: &SpecialRustType,
        generic_types: &[String],
    ) -> Result<String, RustTypeFormatError> {
        Ok(match special_ty {
            SpecialRustType::Vec(rtype) | SpecialRustType::Array(rtype, _) | SpecialRustType::Slice(rtype)=> format!("list[{}]", self.format_type(rtype, generic_types)?),
            SpecialRustType::Option(rtype) => {
                format!("{}|None", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::HashMap(rtype1, rtype2) => format!(
                "dict[{}]{}",
                self.format_type(rtype1, generic_types)?,
                self.format_type(rtype2, generic_types)?
            ),
            SpecialRustType::Unit => "()".into(),
            SpecialRustType::String => "str".into(),
            SpecialRustType::Char => "str".into(), // Python
            SpecialRustType::I8
            | SpecialRustType::U8
            | SpecialRustType::U16
            | SpecialRustType::I32
            | SpecialRustType::I16
            | SpecialRustType::ISize
            | SpecialRustType::USize => "int".into(),
            SpecialRustType::U32 => "int".into(), // TODO consider typing.Annotated[int, annotated_types.Gt(0)]
            SpecialRustType::I54 | SpecialRustType::I64 => "int".into(),
            SpecialRustType::U53 | SpecialRustType::U64 => "int".into(),
            SpecialRustType::Bool => "bool".into(),
            SpecialRustType::F32 => "float".into(),
            SpecialRustType::F64 => "float".into(),
        })
    }

    fn begin_file(&mut self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w)?;
        // TODO write imports if needed
        Ok(())
    }

    fn write_type_alias(&mut self, w: &mut dyn Write, ty: &RustTypeAlias) -> std::io::Result<()> {
        write_comments(w, 0, &ty.comments)?;

        writeln!(
            w,
            "{} = {}\n",
            &ty.id.original,
            self.format_type(&ty.r#type, &[])
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        )?;

        Ok(())
    }

    fn write_struct(&mut self, w: &mut dyn Write, rs: &RustStruct) -> std::io::Result<()> {
        write_comments(w, 0, &rs.comments)?;
        writeln!(
            w,
            "class {}:",
            &rs.id.renamed
        )?;

        rs.fields
            .iter()
            .try_for_each(|f| self.write_field(w, f, rs.generic_types.as_slice()))?;

        writeln!(w)?;
        Ok(())
    }
}

impl Python {
    fn write_enum(
        &mut self,
        w: &mut dyn Write,
        e: &RustEnum,
        custom_structs: &HashSet<&str>,
    ) -> std::io::Result<()> {
        panic!("Enums are not supported in python") // TODO
    }

    fn write_field(
        &mut self,
        w: &mut dyn Write,
        field: &RustField,
        generic_types: &[String],
    ) -> std::io::Result<()> {
        write_comments(w, 1, &field.comments)?;

        let type_name = match field.type_override(SupportedLanguage::Python) {
            Some(type_override) => type_override.to_owned(),
            None => self
                .format_type(&field.ty, generic_types)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?,
        };

        let formatted_renamed_id = format!("{:?}", &field.id.renamed);
        let renamed_id = &formatted_renamed_id[1..formatted_renamed_id.len() - 1];

        if field.id.renamed != field.id.original {
            writeln!(w, "\t@JsonProperty(\"{}\")", renamed_id)?;
        }
        writeln!(
            w,
            "\t{}: {}{}",
            field.id.original.to_string(),
            type_name,
            field.ty.is_optional().then_some("|None").unwrap_or_default(),
        )?;

        Ok(())
    }
}

fn write_comment(w: &mut dyn Write, indent: usize, comment: &str) -> std::io::Result<()> {
    writeln!(w, "{}# {}", "\t".repeat(indent), comment)?;
    Ok(())
}

fn write_comments(w: &mut dyn Write, indent: usize, comments: &[String]) -> std::io::Result<()> {
    comments
        .iter()
        .try_for_each(|comment| write_comment(w, indent, comment))
}
