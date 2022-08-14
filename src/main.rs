use glutin::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopProxy},
    platform::{unix::RawHandle, ContextTraitExt},
    window::Window,
    ContextWrapper, PossiblyCurrent,
};
use libmpv_sys::*;
use std::{
    ffi::{c_void, CStr, CString},
    mem::transmute,
    os::raw::c_char,
    ptr::{null, null_mut},
};

// Some of this code copied/modified from https://github.com/grovesNL/glow/blob/main/examples/hello/src/main.rs
// The rest of it is from https://github.com/mpv-player/mpv-examples/blob/master/libmpv/sdl/main.c
// And some other parts are https://github.com/fltk-rs/demos/blob/master/libmpv/src/sys_main.rs.
// In other words, I am not very creative.

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

#[derive(Debug)]
enum MPVEvent {
    MPVRenderUpdate,
    MPVEventUpdate,
}

unsafe extern "C" fn get_proc_addr(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let rust_name = CStr::from_ptr(name).to_str().unwrap();
    // println!("begin get_proc_addr with name {}", rust_name);
    let window: &ContextWrapper<PossiblyCurrent, Window> = std::mem::transmute(ctx);
    let addr = window.get_proc_address(rust_name) as *mut _;
    // println!("end get_proc_addr {:?}", addr);
    addr
}
unsafe extern "C" fn on_mpv_event(ctx: *mut c_void) {
    let event_proxy: &EventLoopProxy<MPVEvent> = std::mem::transmute(ctx);
    event_proxy
        .send_event(MPVEvent::MPVEventUpdate)
        .expect("Failed to send event update to render loop");
}
unsafe extern "C" fn on_mpv_render_update(ctx: *mut c_void) {
    let event_proxy: &EventLoopProxy<MPVEvent> = std::mem::transmute(ctx);
    event_proxy
        .send_event(MPVEvent::MPVRenderUpdate)
        .expect("Failed to send render update to render loop");
}

fn main() {
    let (gl, shader_version, window, evloop) = unsafe {
        let evloop = glutin::event_loop::EventLoopBuilder::<MPVEvent>::with_user_event().build();
        let window_builder = glutin::window::WindowBuilder::new()
            .with_title("true")
            .with_inner_size(glutin::dpi::LogicalSize::new(WIDTH, HEIGHT));
        let window = glutin::ContextBuilder::new()
            .with_vsync(true)
            .build_windowed(window_builder, &evloop)
            .expect("Failed to build glutin window")
            .make_current()
            .expect("Failed to make window current");
        let gl = glow::Context::from_loader_function(|l| window.get_proc_address(l) as *const _);
        (gl, "#version 410", window, evloop)
    };

    let mpv = unsafe {
        let mpv = mpv_create();
        assert!(!mpv.is_null(), "MPV failed to create!");
        mpv
    };

    let mut mpv_gl: *mut mpv_render_context = null_mut();
    unsafe {
        assert!(mpv_initialize(mpv) == 0, "MPV failed to initialise!");
    };
    unsafe {
        println!("raw handle {:?}", window.context().raw_handle());
    }

    let mut mpv_render_params = unsafe {
        vec![
            mpv_render_param {
                type_: mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                data: transmute(MPV_RENDER_API_TYPE_OPENGL),
            },
            mpv_render_param {
                type_: mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: transmute(&mut mpv_opengl_init_params {
                    get_proc_address: Some(get_proc_addr),
                    get_proc_address_ctx: std::mem::transmute(&window), /*window.context().raw_handle()
                                                                            RawHandle::Glx(addr) => addr,
                                                                            RawHandle::Egl(addr) => panic!("EGL not supported at this time"),
                                                                        } as *mut _,*/
                    extra_exts: null(),
                }),
            },
            mpv_render_param {
                type_: mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
                data: transmute(&mut 1),
            },
            mpv_render_param {
                // end??
                type_: 0,
                data: null_mut(),
            },
        ]
    };

    unsafe {
        assert!(
            mpv_render_context_create(&mut mpv_gl, mpv, mpv_render_params.as_mut_ptr()) == 0,
            "MPV failed to create the render context!"
        )
    };

    // Handle custom MPV events with this
    let event_proxy = evloop.create_proxy();

    // Setup wakeup callback

    unsafe {
        mpv_set_wakeup_callback(mpv, Some(on_mpv_event), std::mem::transmute(&event_proxy));
        // Setup update callback
        mpv_render_context_set_update_callback(
            mpv_gl,
            Some(on_mpv_render_update),
            std::mem::transmute(&event_proxy),
        )
    }

    // Open input file

    let args: Vec<String> = std::env::args().collect();

    let mut mpd_cmd_args: Vec<*const c_char> = vec![
        "loadfile\0".as_ptr() as _,
        CString::new(
            match args.get(1) {
                Some(fname) => fname,
                None => panic!("missing filename as first argument"),
            }
            .as_str(),
        )
        .unwrap()
        .into_raw(),
        null(),
    ];
    unsafe { mpv_command_async(mpv, 0, mpd_cmd_args.as_mut_ptr() as *mut *const _) };

    // https://github.com/grovesNL/glow/blob/main/examples/hello/src/main.rs
    evloop.run(move |event, _, ctrl_flow| {
        *ctrl_flow = ControlFlow::Wait;
        match event {
            Event::LoopDestroyed => {
                return;
            }
            Event::MainEventsCleared => window.window().request_redraw(),
            Event::RedrawRequested(_) => {
                mpv_render_params = unsafe {
                    vec![
                        mpv_render_param {
                            type_: mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
                            data: transmute(&mut mpv_opengl_fbo {
                                fbo: 0,
                                w: WIDTH as i32,
                                h: HEIGHT as i32,
                                internal_format: 0,
                            }),
                        },
                        // Why does MPV render upside down by default ):
                        mpv_render_param {
                            type_: mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
                            data: transmute(&mut 1),
                        },
                        mpv_render_param {
                            type_: mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
                            data: transmute(&mut 1),
                        },
                        mpv_render_param {
                            // end??
                            type_: 0,
                            data: null_mut(),
                        },
                    ]
                };
                unsafe {
                    mpv_render_context_render(mpv_gl, mpv_render_params.as_mut_ptr());
                }
                window.swap_buffers().unwrap();
            }
            Event::WindowEvent { window_id, event } => match event {
                WindowEvent::CloseRequested => unsafe {
                    mpv_render_context_free(mpv_gl);
                    mpv_detach_destroy((mpv));
                    *ctrl_flow = ControlFlow::Exit;
                },
                _ => {}
            },
            Event::UserEvent(ue) => match ue {
                MPVEvent::MPVRenderUpdate => {
                    unsafe {
                        mpv_render_context_update(mpv_gl);
                    }
                    window.window().request_redraw();
                }
                MPVEvent::MPVEventUpdate => {
                    while true {
                        let mpv_event = unsafe { mpv_wait_event(mpv, 0.0) };
                        match unsafe { (*mpv_event).event_id } {
                            mpv_event_id_MPV_EVENT_NONE => break,
                            mpv_event_id_MPV_EVENT_LOG_MESSAGE => {
                                println!("mpv_log {}", unsafe {
                                    CStr::from_ptr((*mpv_event).data as *const i8)
                                        .to_str()
                                        .unwrap()
                                });
                            }
                            _ => {}
                        }
                        unsafe {
                            println!(
                                "mpv_event {}",
                                CStr::from_ptr(mpv_event_name((*mpv_event).event_id))
                                    .to_str()
                                    .unwrap()
                            )
                        }
                    }
                }
            },
            _ => {} /*Event::DeviceEvent { device_id, event } => todo!(),
                    Event::UserEvent(_) => todo!(),
                    Event::Suspended => todo!(),
                    Event::Resumed => todo!(),
                    Event::RedrawEventsCleared => todo!(),*/
        }
    })
}
