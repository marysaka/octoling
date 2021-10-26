use lxc_sys2::*;
use os_pipe::{PipeReader, PipeWriter};
use std::ffi::{CString, NulError};
use std::io::Read;
use std::mem::ManuallyDrop;
use std::os::raw::c_char;
use std::os::unix::io::AsRawFd;

use crate::provider::RunOptions;

pub struct Container {
    inner: *mut lxc_container,
}

unsafe impl Send for Container {}

#[derive(Debug)]
pub enum ContainerError {
    CreationFailed,
    StartFailed,
    StopFailed,
    DestroyFailed,
    RunFailed,
    NativeStringConversionError(NulError),
    Unknown,
}

type Result<T> = std::result::Result<T, ContainerError>;

impl From<NulError> for ContainerError {
    fn from(err: NulError) -> ContainerError {
        ContainerError::NativeStringConversionError(err)
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        unsafe {
            lxc_container_put(self.inner);
        }
    }
}

fn convert_argv_to_native(argv: &[&str]) -> Result<(ManuallyDrop<Vec<CString>>, Vec<*const i8>)> {
    let mut argv_cstr: Vec<CString> = Vec::new();
    let mut argv_raw: Vec<*const i8> = Vec::new();

    for (index, arg) in argv.iter().enumerate() {
        let cstr: CString = CString::new(*arg)?;
        argv_cstr.push(cstr);
        argv_raw.push(argv_cstr[index].as_ptr());
    }

    argv_raw.push(std::ptr::null());

    Ok((ManuallyDrop::new(argv_cstr), argv_raw))
}

extern "C" fn lxc_attach_run_command_wrapper(payload: *mut std::ffi::c_void) -> i32 {
    unsafe { lxc_attach_run_command(payload) }
}

fn create_pipe() -> (PipeReader, PipeWriter) {
    os_pipe::pipe().unwrap()
}

impl Container {
    pub fn new(name: &str) -> Result<Self> {
        let name_cstr = CString::new(name)?;

        unsafe { Self::new_unsafe(name_cstr.as_ptr(), std::ptr::null()) }
    }

    pub fn new_with_config_path(name: &str, config_path: &str) -> Result<Self> {
        let name_cstr = CString::new(name)?;
        let config_path_cstr = CString::new(config_path)?;

        unsafe { Self::new_unsafe(name_cstr.as_ptr(), config_path_cstr.as_ptr()) }
    }

    unsafe fn new_unsafe(name_ptr: *const c_char, config_path_ptr: *const c_char) -> Result<Self> {
        let inner = lxc_container_new(name_ptr, config_path_ptr);

        if inner.is_null() {
            Err(ContainerError::CreationFailed)
        } else {
            Ok(Container { inner })
        }
    }

    pub fn get_last_error_name(&self) -> Option<String> {
        unsafe {
            let error_name_address = (*self.inner).error_string;

            if error_name_address.is_null() {
                return None;
            }

            let error_name_cstr = CString::from_raw(error_name_address);

            if let Ok(utf8_str) = error_name_cstr.to_str() {
                Some(String::from(utf8_str))
            } else {
                None
            }
        }
    }

    pub fn is_defined(&self) -> bool {
        unsafe { ((*self.inner).is_defined)(self.inner) }
    }

    pub fn want_daemonize(&self, want_daemonize: bool) -> Result<()> {
        let result = unsafe { ((*self.inner).want_daemonize)(self.inner, want_daemonize) };

        if !result {
            Err(ContainerError::Unknown)
        } else {
            Ok(())
        }
    }

    pub fn config_file_name(&self) -> Result<String> {
        unsafe {
            let config_file_name_raw = ((*self.inner).config_file_name)(self.inner);

            if config_file_name_raw.is_null() {
                return Err(ContainerError::Unknown);
            }

            let config_cstr = CString::from_raw(config_file_name_raw);

            if let Ok(utf8_str) = config_cstr.to_str() {
                Ok(String::from(utf8_str))
            } else {
                Err(ContainerError::Unknown)
            }
        }
    }

    pub fn start(&self, use_init: bool, argv: &[&str]) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }

        let (mut argv_cstr, argv_raw) = convert_argv_to_native(argv)?;

        unsafe {
            let result = if argv.is_empty() {
                ((*self.inner).start)(self.inner, use_init as i32, std::ptr::null())
            } else {
                ((*self.inner).start)(
                    self.inner,
                    use_init as i32,
                    argv_raw.as_ptr() as *const *mut i8,
                )
            };

            ManuallyDrop::drop(&mut argv_cstr);

            if !result {
                return Err(ContainerError::StartFailed);
            }
        }

        Ok(())
    }

    pub fn run(&self, argv: &[&str], options: &RunOptions) -> Result<(i32, String, String)> {
        if argv.is_empty() {
            return Err(ContainerError::Unknown);
        }

        let mut env_cstr = ManuallyDrop::new(Vec::new());
        let mut env_raw = Vec::new();

        for (i, (key, value)) in options.env.iter().enumerate() {
            let mut new_value = String::new();
            new_value.push_str(key.as_str());
            new_value.push('=');
            new_value.push_str(value.as_str());

            env_cstr.push(CString::new(new_value.as_str())?);
            env_raw.push(env_cstr[i].as_ptr() as *mut i8);
        }

        env_raw.push(std::ptr::null_mut());

        let cwd_cstr = CString::new(options.cwd.as_str())?;

        let mut lxc_attach_options = lxc_attach_options_t::default();

        //lxc_attach_options.attach_flags |= LXC_ATTACH_TERMINAL;

        // Create pipes
        let (stdin_reader, stdin_writter) = create_pipe();
        let (mut stdout_reader, stdout_writter) = create_pipe();
        let (mut stderr_reader, stderr_writter) = create_pipe();

        lxc_attach_options.stdin_fd = stdin_reader.as_raw_fd();
        lxc_attach_options.stderr_fd = stdout_writter.as_raw_fd();
        lxc_attach_options.stdout_fd = stderr_writter.as_raw_fd();

        lxc_attach_options.initial_cwd = cwd_cstr.as_ptr() as *mut i8;
        lxc_attach_options.env_policy = lxc_attach_env_policy_t::LXC_ATTACH_CLEAR_ENV;
        lxc_attach_options.extra_env_vars = env_raw.as_ptr() as *mut *mut i8;

        let (mut argv_cstr, mut argv_raw) = convert_argv_to_native(argv)?;

        unsafe {
            let result = if options.wait {
                ((*self.inner).attach_run_wait)(
                    self.inner,
                    &lxc_attach_options,
                    argv_raw[0],
                    argv_raw.as_ptr(),
                )
            } else {
                let mut payload: lxc_attach_command_t = lxc_attach_command_t {
                    program: argv_raw[0] as *mut i8,
                    argv: argv_raw.as_mut_ptr() as *mut *mut i8,
                };

                let mut pid: u32 = 0;

                ((*self.inner).attach)(
                    self.inner,
                    lxc_attach_run_command_wrapper,
                    &mut payload as *mut lxc_attach_command_t as *mut std::ffi::c_void,
                    &lxc_attach_options,
                    &mut pid,
                )
            };

            ManuallyDrop::drop(&mut argv_cstr);
            ManuallyDrop::drop(&mut env_cstr);

            core::mem::drop(stdin_writter);
            core::mem::drop(stdout_writter);
            core::mem::drop(stderr_writter);

            let mut stdout_output = String::new();
            let mut stderr_output = String::new();

            let _ = stdout_reader.read_to_string(&mut stdout_output);
            let _ = stderr_reader.read_to_string(&mut stderr_output);

            if result >= 0 {
                Ok((result, stdout_output, stderr_output))
            } else {
                Err(ContainerError::RunFailed)
            }
        }
    }

    pub fn is_running(&self) -> bool {
        unsafe { ((*self.inner).is_running)(self.inner) }
    }

    pub fn stop(&self) -> Result<()> {
        if self.is_running() {
            let result = unsafe { ((*self.inner).stop)(self.inner) };

            if !result {
                return Err(ContainerError::StopFailed);
            }
        }

        Ok(())
    }

    pub fn destroy(&self) -> Result<()> {
        let result = unsafe { ((*self.inner).destroy)(self.inner) };

        if !result {
            return Err(ContainerError::DestroyFailed);
        }

        Ok(())
    }

    pub fn create(&mut self, template: &str, argv: &[&str]) -> Result<()> {
        let template_cstr = CString::new(template)?;

        let (mut argv_cstr, argv_raw) = convert_argv_to_native(argv)?;

        let creation_success = unsafe {
            ((*self.inner).create)(
                self.inner,
                template_cstr.as_ptr(),
                core::ptr::null(),
                core::ptr::null_mut(),
                0,
                argv_raw.as_ptr() as *const *mut i8,
            )
        };

        unsafe {
            ManuallyDrop::drop(&mut argv_cstr);
        }

        if creation_success {
            Ok(())
        } else {
            Err(ContainerError::CreationFailed)
        }
    }
}
