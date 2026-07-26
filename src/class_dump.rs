//! Full IL2CPP class enumeration for reverse-engineering diagnostics.

use std::ffi::{c_char, c_void, CStr};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use crate::compat::Sdk;

#[repr(C)]
struct FieldInfoCompat {
    name: *const c_char,
    type_: *const c_void,
}

#[repr(C)]
struct MethodInfoCompat {
    method_pointer: usize,
    virtual_method_pointer: usize,
    invoker_method: usize,
    name: *const c_char,
    klass: *mut c_void,
    return_type: *const c_void,
    parameters: *mut c_void,
    _union1: usize,
    _union2: usize,
    token: u32,
    flags: u16,
    iflags: u16,
    slot: u16,
    parameters_count: u8,
}

struct DumpContext {
    domain_get: unsafe extern "C" fn() -> *mut c_void,
    domain_get_assemblies: unsafe extern "C" fn(*const c_void, *mut usize) -> *mut *const c_void,
    assembly_get_image: unsafe extern "C" fn(*const c_void) -> *const c_void,
    image_get_name: unsafe extern "C" fn(*const c_void) -> *const c_char,
    image_get_class_count: unsafe extern "C" fn(*const c_void) -> usize,
    image_get_class: unsafe extern "C" fn(*const c_void, usize) -> *mut c_void,
    class_get_name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    class_get_namespace: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    class_get_declaring_type: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    class_get_fields: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void,
    type_get_name: unsafe extern "C" fn(*const c_void) -> *mut c_char,
    method_get_param_count: unsafe extern "C" fn(*const c_void) -> u32,
    method_get_param: unsafe extern "C" fn(*const c_void, u32) -> *const c_void,
    method_get_param_name: unsafe extern "C" fn(*const c_void, u32) -> *const c_char,
    il2cpp_free: unsafe extern "C" fn(*mut c_void),
}

impl DumpContext {
    fn resolve() -> Option<Self> {
        let sdk = Sdk::get();
        // SAFETY: Resolved symbols are IL2CPP C API exports with the signatures declared here.
        Some(unsafe {
            Self {
                domain_get: std::mem::transmute(sdk.resolve_symbol("il2cpp_domain_get")?),
                domain_get_assemblies: std::mem::transmute(sdk.resolve_symbol("il2cpp_domain_get_assemblies")?),
                assembly_get_image: std::mem::transmute(sdk.resolve_symbol("il2cpp_assembly_get_image")?),
                image_get_name: std::mem::transmute(sdk.resolve_symbol("il2cpp_image_get_name")?),
                image_get_class_count: std::mem::transmute(sdk.resolve_symbol("il2cpp_image_get_class_count")?),
                image_get_class: std::mem::transmute(sdk.resolve_symbol("il2cpp_image_get_class")?),
                class_get_name: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_get_name")?),
                class_get_namespace: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_get_namespace")?),
                class_get_declaring_type: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_get_declaring_type")?),
                class_get_fields: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_get_fields")?),
                type_get_name: std::mem::transmute(sdk.resolve_symbol("il2cpp_type_get_name")?),
                method_get_param_count: std::mem::transmute(sdk.resolve_symbol("il2cpp_method_get_param_count")?),
                method_get_param: std::mem::transmute(sdk.resolve_symbol("il2cpp_method_get_param")?),
                method_get_param_name: std::mem::transmute(sdk.resolve_symbol("il2cpp_method_get_param_name")?),
                il2cpp_free: std::mem::transmute(sdk.resolve_symbol("il2cpp_free")?),
            }
        })
    }

    fn static_str(&self, ptr: *const c_char) -> String {
        if ptr.is_null() {
            return "?".to_string();
        }
        // SAFETY: Pointer is a null-terminated static string from IL2CPP metadata.
        unsafe { CStr::from_ptr(ptr).to_str().unwrap_or("?").to_string() }
    }

    /// Build a typed parameter list like `System.Int32 skillId, Gallop.SkillTips tips`.
    /// Falls back gracefully on null names/types so a single bad entry never aborts the dump.
    fn method_signature(&self, method: *const c_void) -> String {
        // SAFETY: `method` is a valid IL2CPP MethodInfo* from class method enumeration.
        let count = unsafe { (self.method_get_param_count)(method) };
        let mut parts = Vec::with_capacity(count as usize);
        for i in 0..count {
            // SAFETY: `i` is in range [0, count) for this method's parameters.
            let type_ptr = unsafe { (self.method_get_param)(method, i) };
            let type_name = self.type_name(type_ptr);
            // SAFETY: `i` is in range; returned pointer is a static metadata string or null.
            let name_ptr = unsafe { (self.method_get_param_name)(method, i) };
            let param_name = self.static_str(name_ptr);
            parts.push(format!("{} {}", type_name, param_name));
        }
        parts.join(", ")
    }

    fn type_name(&self, type_ptr: *const c_void) -> String {
        if type_ptr.is_null() {
            return "void".to_string();
        }
        // SAFETY: `type_ptr` is from IL2CPP metadata. Returned string is allocated by IL2CPP.
        unsafe {
            let name_ptr = (self.type_get_name)(type_ptr);
            if name_ptr.is_null() {
                return "?".to_string();
            }
            let name = CStr::from_ptr(name_ptr).to_str().unwrap_or("?").to_string();
            (self.il2cpp_free)(name_ptr.cast());
            name
        }
    }
}

/// Build a dotted name chain for nested classes (e.g. `MasterSkillData.SkillData`).
fn qualified_name(ctx: &DumpContext, klass: *mut c_void) -> String {
    // SAFETY: Class pointer is valid IL2CPP metadata.
    let name = ctx.static_str(unsafe { (ctx.class_get_name)(klass) });
    // SAFETY: Class pointer is valid IL2CPP metadata.
    let declaring = unsafe { (ctx.class_get_declaring_type)(klass) };
    if declaring.is_null() || declaring == klass {
        return name;
    }
    format!("{}.{}", qualified_name(ctx, declaring), name)
}

pub fn dump_all_classes() {
    let Some(ctx) = DumpContext::resolve() else {
        hlog_error!("Class dump failed: could not resolve IL2CPP enumeration symbols");
        return;
    };

    match dump_all_classes_inner(&ctx) {
        Ok((path, assemblies, classes)) => {
            hlog_info!(
                "Dumped {} IL2CPP classes from {} assemblies to {}",
                classes,
                assemblies,
                path.display()
            );
        }
        Err(err) => hlog_error!("Class dump failed: {}", err),
    }
}

/// The .NET base class library assemblies. Introspecting some of their runtime
/// types (e.g. `mscorlib` `MulticastDelegate`) segfaults the IL2CPP metadata
/// APIs, and none of it is game code worth reverse-engineering. Skipping them
/// lets the dump complete cleanly over every game assembly.
fn is_bcl_image(name: &str) -> bool {
    matches!(name, "mscorlib.dll" | "netstandard.dll")
        || name.starts_with("System")
        || name.starts_with("Mono.")
        || name.starts_with("Microsoft.")
}

fn dump_all_classes_inner(ctx: &DumpContext) -> std::io::Result<(PathBuf, usize, usize)> {
    // SAFETY: IL2CPP runtime is initialized before plugin UI callbacks run.
    let domain = unsafe { (ctx.domain_get)() };
    if domain.is_null() {
        return Err(std::io::Error::other("il2cpp_domain_get returned null"));
    }

    let mut assembly_count = 0usize;
    // SAFETY: Domain pointer is valid; IL2CPP writes the assembly count.
    let assemblies = unsafe { (ctx.domain_get_assemblies)(domain, &mut assembly_count) };
    if assemblies.is_null() {
        return Err(std::io::Error::other("il2cpp_domain_get_assemblies returned null"));
    }

    let path = output_path();
    let file = File::create(&path)?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "# IL2CPP class dump")?;
    writeln!(writer, "# Assemblies: {}", assembly_count)?;

    let mut total_classes = 0usize;
    for assembly_idx in 0..assembly_count {
        // SAFETY: `assemblies` points to an array of `assembly_count` assembly pointers.
        let assembly = unsafe { *assemblies.add(assembly_idx) };
        if assembly.is_null() {
            continue;
        }
        // SAFETY: Assembly pointer is from IL2CPP domain enumeration.
        let image = unsafe { (ctx.assembly_get_image)(assembly) };
        if image.is_null() {
            continue;
        }

        // SAFETY: Image pointer is valid IL2CPP metadata.
        let image_name = ctx.static_str(unsafe { (ctx.image_get_name)(image) });
        if is_bcl_image(&image_name) {
            writeln!(writer, "\n=== Assembly: {} (skipped: .NET BCL) ===", image_name)?;
            writer.flush()?;
            continue;
        }
        // SAFETY: Image pointer is valid IL2CPP metadata.
        let class_count = unsafe { (ctx.image_get_class_count)(image) };
        total_classes += class_count;

        writeln!(writer, "\n=== Assembly: {} ({} classes) ===", image_name, class_count)?;

        for class_idx in 0..class_count {
            // SAFETY: Class index is in range for this image.
            let klass = unsafe { (ctx.image_get_class)(image, class_idx) };
            if klass.is_null() {
                continue;
            }
            dump_class(ctx, &mut writer, klass)?;
            // Flush per class so a crash in IL2CPP introspection never discards
            // prior output and the trailing class header pinpoints the culprit.
            writer.flush()?;
        }
    }

    writer.flush()?;
    Ok((path, assembly_count, total_classes))
}

fn dump_class(ctx: &DumpContext, writer: &mut impl Write, klass: *mut c_void) -> std::io::Result<()> {
    // Build qualified name including declaring type chain for nested classes.
    let qualified = qualified_name(ctx, klass);
    // SAFETY: Class pointer is valid IL2CPP metadata.
    let namespace = ctx.static_str(unsafe { (ctx.class_get_namespace)(klass) });

    if namespace.is_empty() {
        writeln!(writer, "\n[] {}", qualified)?;
    } else {
        writeln!(writer, "\n[{}] {}", namespace, qualified)?;
    }
    dump_fields(ctx, writer, klass)?;
    dump_methods(ctx, writer, klass)?;
    Ok(())
}

fn dump_fields(ctx: &DumpContext, writer: &mut impl Write, klass: *mut c_void) -> std::io::Result<()> {
    let mut iter: *mut c_void = std::ptr::null_mut();
    let mut count = 0usize;
    loop {
        // SAFETY: Iterator follows IL2CPP `void* iter = NULL` convention.
        let field = unsafe { (ctx.class_get_fields)(klass, &mut iter) };
        if field.is_null() {
            break;
        }

        // SAFETY: FieldInfoCompat matches the leading IL2CPP FieldInfo fields used here.
        unsafe {
            let fi = &*(field as *const FieldInfoCompat);
            let field_name = ctx.static_str(fi.name);
            let type_name = ctx.type_name(fi.type_);
            writeln!(writer, "  field: {} {}", type_name, field_name)?;
        }

        count += 1;
        if count >= 500 {
            writeln!(writer, "  ... fields truncated at 500")?;
            break;
        }
    }
    Ok(())
}

fn dump_methods(ctx: &DumpContext, writer: &mut impl Write, klass: *mut c_void) -> std::io::Result<()> {
    let sdk = Sdk::get();
    let mut iter: *mut c_void = std::ptr::null_mut();
    let mut count = 0usize;
    loop {
        let method = sdk.class_get_methods(klass.cast(), &mut iter);
        if method.is_null() {
            break;
        }

        // SAFETY: MethodInfoCompat matches the leading IL2CPP MethodInfo fields used here.
        unsafe {
            let mi = &*(method as *const MethodInfoCompat);
            let method_name = ctx.static_str(mi.name);
            let return_type = ctx.type_name(mi.return_type);
            let params = ctx.method_signature(method.cast());
            writeln!(writer, "  method: {} {}({})", return_type, method_name, params)?;
        }

        count += 1;
        if count >= 500 {
            writeln!(writer, "  ... methods truncated at 500")?;
            break;
        }
    }
    Ok(())
}

fn output_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("il2cpp_classes.txt")))
        .unwrap_or_else(|| PathBuf::from("il2cpp_classes.txt"))
}
