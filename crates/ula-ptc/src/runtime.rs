use std::collections::{HashMap, VecDeque};

use mlua::{Function, Lua, MultiValue, StdLib, Thread, Value, Variadic, thread::ThreadStatus};

use crate::errors::Error;

pub type ToolCallArgs = String;

pub struct Runtime {
    lua: Lua,
    threads: HashMap<u32, Thread>,
    pending: VecDeque<(u32, Thread, Option<ToolCallArgs>)>,
}

impl Runtime {
    pub fn new() -> Result<Self, Error> {
        let lua = Lua::new_with(
            StdLib::MATH | StdLib::COROUTINE | StdLib::STRING | StdLib::TABLE | StdLib::UTF8,
            Default::default(),
        )?;

        lua.globals()
            .set("print", print_to_stderr(&lua).map_err(Error::from)?)?;

        Ok(Self {
            lua,
            threads: HashMap::new(),
            pending: VecDeque::new(),
        })
    }

    pub fn inject(&self, code: &str) -> Result<(), Error> {
        self.lua.load(code).set_name("ula-tools").exec()?;
        Ok(())
    }

    pub fn execute(&mut self, id: u32, code: &str) -> Result<(), Error> {
        let function: Function = self
            .lua
            .load(code)
            .set_name("ula-ptc")
            .into_function()?;
        let thread = self.lua.create_thread(function)?;
        self.pending.push_back((id, thread, None));
        Ok(())
    }

    pub fn resume(&mut self, id: u32, args: Option<ToolCallArgs>) -> Result<(), Error> {
        let thread = self.threads.remove(&id).ok_or_else(|| Error::Lua {
            msg: format!("no suspended thread for call {id}"),
        })?;
        self.pending.push_back((id, thread, args));
        Ok(())
    }

    pub fn cancel(&mut self, id: u32) {
        self.threads.remove(&id);
        self.pending.retain(|(pending, ..)| *pending != id);
    }

    pub fn poll(&mut self) -> Option<LuaEvent> {
        let (id, thread, args) = self.pending.pop_front()?;

        let result = match args {
            Some(args) => match self.lua.create_string(&args) {
                Ok(args) => thread.resume::<MultiValue>(Value::String(args)),
                Err(e) => Err(e),
            },
            None => thread.resume::<MultiValue>(Value::Nil),
        };

        match result {
            Ok(values) => match thread.status() {
                ThreadStatus::Finished => Some(LuaEvent::Finished {
                    result: extract_output(&values),
                }),
                ThreadStatus::Resumable => match extract_toolcall(&values) {
                    Some((request_type, payload)) => {
                        self.threads.insert(id, thread);
                        Some(LuaEvent::ToolCall {
                            id,
                            request_type,
                            payload,
                        })
                    }
                    None => Some(LuaEvent::Error {
                        message: String::from("coroutine yielded without a tool call"),
                    }),
                },
                status => Some(LuaEvent::Error {
                    message: format!("unexpected thread status: {status:?}"),
                }),
            },
            Err(e) => Some(LuaEvent::Error {
                message: e.to_string(),
            }),
        }
    }
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

pub enum LuaEvent {
    ToolCall {
        id: u32,
        request_type: String,
        payload: HashMap<String, String>,
    },
    Finished {
        result: String,
    },
    Error {
        message: String,
    },
}

fn extract_toolcall(values: &MultiValue) -> Option<(String, HashMap<String, String>)> {
    let table = values.front()?.as_table()?;
    let name: String = table.get("name").ok()?;
    let params_table: mlua::Table = table.get("params").ok()?;
    let mut params = HashMap::new();
    for (key, value) in params_table.pairs::<String, String>().flatten() {
        params.insert(key, value);
    }
    Some((name, params))
}

fn extract_output(values: &MultiValue) -> String {
    let parts: Vec<String> = values
        .iter()
        .filter_map(|v| v.to_string().ok())
        .collect();
    parts.join("\n")
}
