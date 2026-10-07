use std::collections::HashMap;

use mlua::{Function, Lua, MultiValue, StdLib, Thread, Value, Variadic, thread::ThreadStatus};

use crate::errors::PTCError;

pub enum LuaEvent {
    ToolCall {
        request_type: String,
        payload: HashMap<String, String>,
    },
    Finished {
        result: String,
    },
}

pub struct Runtime {
    lua: Lua,
    thread: Option<Thread>,
}

impl Runtime {
    pub fn new() -> Result<Self, PTCError> {
        let lua = Lua::new_with(
            StdLib::MATH | StdLib::COROUTINE | StdLib::STRING | StdLib::TABLE | StdLib::UTF8,
            Default::default(),
        )?;

        lua.globals().set("print", print_to_stderr(&lua)?)?;

        Ok(Self { lua, thread: None })
    }

    pub fn inject(&self, code: &str) -> Result<(), PTCError> {
        self.lua.load(code).set_name("ula-tools").exec()?;
        Ok(())
    }

    pub fn execute(&mut self, code: &str) -> Result<LuaEvent, PTCError> {
        let function: Function = self.lua.load(code).set_name("ula-ptc").into_function()?;
        let thread = self.lua.create_thread(function)?;
        self.thread = Some(thread);
        self.advance(Value::Nil)
    }

    pub fn resume(&mut self, output: &str) -> Result<LuaEvent, PTCError> {
        let output = self.lua.create_string(output)?;
        self.advance(Value::String(output))
    }

    fn advance(&mut self, arg: Value) -> Result<LuaEvent, PTCError> {
        let thread = self.thread.take().ok_or_else(|| PTCError::Internal {
            msg: "no suspended coroutine".into(),
        })?;

        let values = thread.resume::<MultiValue>(arg)?;
        match thread.status() {
            ThreadStatus::Finished => Ok(LuaEvent::Finished {
                result: extract_output(&values),
            }),
            ThreadStatus::Resumable => {
                self.thread = Some(thread);
                let (request_type, payload) = extract_toolcall(&values)?;
                Ok(LuaEvent::ToolCall {
                    request_type,
                    payload,
                })
            }
            status => PTCError::Internal {
                msg: format!("unexpected thread status: {status:?}"),
            }
            .raise(),
        }
    }
}

fn extract_toolcall(values: &MultiValue) -> Result<(String, HashMap<String, String>), PTCError> {
    let table = values
        .front()
        .and_then(Value::as_table)
        .ok_or_else(|| PTCError::Internal {
            msg: "coroutine yielded without a tool call".into(),
        })?;
    let name: String = table.get("name")?;
    let params: mlua::Table = table.get("params")?;
    let payload = params
        .pairs::<String, mlua::Value>()
        .map(|pair| {
            let (key, value) = pair.map_err(PTCError::from)?;
            Ok::<_, PTCError>((key, value.to_string().map_err(PTCError::from)?))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok((name, payload))
}

fn extract_output(values: &MultiValue) -> String {
    values
        .iter()
        .filter_map(|v| v.to_string().ok())
        .collect::<Box<[_]>>()
        .join("\n")
}

fn print_to_stderr(lua: &Lua) -> Result<mlua::Function, mlua::Error> {
    lua.create_function(|_, args: Variadic<Value>| {
        let line: Vec<_> = args
            .iter()
            .filter_map(|value| value.to_string().ok())
            .collect();
        eprintln!("{}", line.join("\t"));
        Ok(())
    })
}
