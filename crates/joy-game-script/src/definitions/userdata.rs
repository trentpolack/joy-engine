// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Adapters for the small subset of mlua userdata registration used by engine and game APIs.

use super::{
    Function,
    LuaClass,
    LuaDefinitions,
    LuaSignature,
    LuaType,
    LuaTypes,
};
use mlua::{
    FromLua,
    FromLuaMulti,
    IntoLua,
    IntoLuaMulti,
    Lua,
    MetaMethod,
    UserDataFields,
    UserDataMethods,
};
use std::marker::PhantomData;

/// Field registration target used by `lua_userdata!`.
#[doc(hidden)]
pub struct LuaFields<'a, T, F> {
    runtime: Option<&'a mut F>,
    description: Option<(&'a mut LuaDefinitions, &'a mut LuaClass)>,
    marker: PhantomData<T>,
}
impl<'a, T, F: UserDataFields<T>> LuaFields<'a, T, F> {
    /// Register fields on a real mlua userdata type.
    pub fn runtime(fields: &'a mut F) -> Self {
        Self {
            runtime: Some(fields),
            description: None,
            marker: PhantomData,
        }
    }
    /// Collect the same field declarations without constructing any userdata.
    pub fn describing(definitions: &'a mut LuaDefinitions, class: &'a mut LuaClass) -> Self {
        Self {
            runtime: None,
            description: Some((definitions, class)),
            marker: PhantomData,
        }
    }
    /// Register a getter, deriving its Lua field type from its Rust result.
    pub fn add_field_method_get<M, R>(&mut self, name: &str, method: M)
    where
        M: Fn(&Lua, &T) -> mlua::Result<R> + 'static,
        R: IntoLua + LuaType,
    {
        self.add_field_method_get_as(name, None, method);
    }
    /// Refine a getter returning a dynamic Lua value, beside the callback that implements it.
    pub fn add_field_method_get_as<M, R>(&mut self, name: &str, kind: Option<&str>, method: M)
    where
        M: Fn(&Lua, &T) -> mlua::Result<R> + 'static,
        R: IntoLua + LuaType,
    {
        if let Some((definitions, class)) = &mut self.description {
            let inferred = R::lua_type(definitions);
            if class.fields.insert(name.to_owned(), kind.unwrap_or(&inferred).to_owned()).is_some() {
                definitions.errors.push(format!("duplicate Lua field `{name}`"));
            }
        }
        if let Some(fields) = &mut self.runtime {
            fields.add_field_method_get(name, method);
        }
    }
    /// Register a setter; the getter remains the field's readable type.
    pub fn add_field_method_set<M, A>(&mut self, name: &str, method: M)
    where
        M: FnMut(&Lua, &mut T, A) -> mlua::Result<()> + 'static,
        A: FromLua + LuaType,
    {
        if let Some((definitions, class)) = &mut self.description {
            let kind = A::lua_type(definitions);
            class.setters.insert(name.to_owned(), kind.clone());
            class.fields.entry(name.to_owned()).or_insert(kind);
        }
        if let Some(fields) = &mut self.runtime {
            fields.add_field_method_set(name, method);
        }
    }
}

/// Method registration target used by `lua_userdata!`.
#[doc(hidden)]
pub struct LuaMethods<'a, T, M> {
    runtime: Option<&'a mut M>,
    description: Option<(&'a mut LuaDefinitions, &'a mut LuaClass)>,
    marker: PhantomData<T>,
}
impl<'a, T, M: UserDataMethods<T>> LuaMethods<'a, T, M> {
    /// Register methods on a real mlua userdata type.
    pub fn runtime(methods: &'a mut M) -> Self {
        Self {
            runtime: Some(methods),
            description: None,
            marker: PhantomData,
        }
    }
    /// Collect the same methods without executing callbacks or borrowing gameplay state.
    pub fn describing(definitions: &'a mut LuaDefinitions, class: &'a mut LuaClass) -> Self {
        Self {
            runtime: None,
            description: Some((definitions, class)),
            marker: PhantomData,
        }
    }
    /// Register an immutable method; names exclude the implicit Lua `self` parameter.
    pub fn add_method<C, A, R>(&mut self, name: &str, signature: LuaSignature, method: C)
    where
        C: Fn(&Lua, &T, A) -> mlua::Result<R> + 'static,
        A: FromLuaMulti + LuaTypes,
        R: IntoLuaMulti + LuaTypes,
    {
        self.describe::<A, R>(name, signature, None, false);
        if let Some(methods) = &mut self.runtime {
            methods.add_method(name, method);
        }
    }
    /// Register a method that may mutate its userdata receiver.
    pub fn add_method_mut<C, A, R>(&mut self, name: &str, signature: LuaSignature, method: C)
    where
        C: FnMut(&Lua, &mut T, A) -> mlua::Result<R> + 'static,
        A: FromLuaMulti + LuaTypes,
        R: IntoLuaMulti + LuaTypes,
    {
        self.describe::<A, R>(name, signature, None, false);
        if let Some(methods) = &mut self.runtime {
            methods.add_method_mut(name, method);
        }
    }
    /// Register an operator implemented as a method on its left operand.
    pub fn add_meta_method<C, A, R>(&mut self, name: MetaMethod, signature: LuaSignature, method: C)
    where
        C: Fn(&Lua, &T, A) -> mlua::Result<R> + 'static,
        A: FromLuaMulti + LuaTypes,
        R: IntoLuaMulti + LuaTypes,
    {
        self.describe::<A, R>(name.name(), signature, Some(name), false);
        if let Some(methods) = &mut self.runtime {
            methods.add_meta_method(name, method);
        }
    }
    /// Register an operator that accepts either operand as its receiver (for example scalar times vector).
    pub fn add_meta_function<C, A, R>(&mut self, name: MetaMethod, signature: LuaSignature, function: C)
    where
        C: Fn(&Lua, A) -> mlua::Result<R> + 'static,
        A: FromLuaMulti + LuaTypes,
        R: IntoLuaMulti + LuaTypes,
    {
        self.describe::<A, R>(name.name(), signature, Some(name), true);
        if let Some(methods) = &mut self.runtime {
            methods.add_meta_function(name, function);
        }
    }

    fn describe<A: LuaTypes, R: LuaTypes>(
        &mut self,
        name: &str,
        signature: LuaSignature,
        operator: Option<MetaMethod>,
        function: bool,
    ) {
        let Some((definitions, class)) = &mut self.description else {
            return;
        };
        match Function::describe::<A, R>(signature, definitions) {
            Ok(mut description) => {
                if let Some(operator) = operator {
                    let kind = match operator {
                        MetaMethod::Add => "add",
                        MetaMethod::Sub => "sub",
                        MetaMethod::Mul => "mul",
                        MetaMethod::Div => "div",
                        MetaMethod::Unm => "unm",
                        // LuaLS already permits equality and tostring for userdata; they have no corresponding operator annotations.
                        MetaMethod::Eq | MetaMethod::ToString => return,
                        _ => {
                            definitions.errors.push(format!("unsupported LuaLS operator `{name}`"));
                            return;
                        }
                    };
                    if function && !description.arguments.is_empty() {
                        description.arguments.remove(0);
                    }
                    class.operators.push((kind.to_owned(), description));
                } else if class.methods.insert(name.to_owned(), description).is_some() {
                    definitions.errors.push(format!("duplicate Lua method `{name}`"));
                }
            }
            Err(error) => definitions.errors.push(format!("{name}: {error}")),
        }
    }
}

/// Declare userdata once for both mlua and LuaLS. Bodies use typed field/method adapters; callbacks are never run
/// during generation. Type names are Lua class names, not global constructors. Referenced types are collected recursively.
#[macro_export]
macro_rules! lua_userdata {
    ($ty:ty, $name:literal, fields |$fields:ident| $field_body:block methods |$methods:ident| $method_body:block) => {
        impl $crate::mlua::UserData for $ty {
            fn add_fields<F: $crate::mlua::UserDataFields<Self>>(fields: &mut F) {
                let mut $fields = $crate::definitions::LuaFields::<Self, F>::runtime(fields);
                $field_body
            }
            fn add_methods<M: $crate::mlua::UserDataMethods<Self>>(methods: &mut M) {
                let mut $methods = $crate::definitions::LuaMethods::<Self, M>::runtime(methods);
                $method_body
            }
        }
        impl $crate::LuaType for $ty {
            fn lua_type(definitions: &mut $crate::LuaDefinitions) -> String {
                definitions.collect_class::<Self>($name, |definitions, class| {
                    {
                        let mut $fields =
                            $crate::definitions::LuaFields::<Self, $crate::mlua::UserDataRegistry<Self>>::describing(
                                definitions,
                                class,
                            );
                        $field_body
                    }
                    {
                        let mut $methods =
                            $crate::definitions::LuaMethods::<Self, $crate::mlua::UserDataRegistry<Self>>::describing(
                                definitions,
                                class,
                            );
                        $method_body
                    }
                })
            }
        }
    };
}
