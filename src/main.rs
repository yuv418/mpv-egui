use glow::HasContext;
use glutin::{
    event::{ElementState, Event, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopProxy},
    platform::{unix::RawHandle, ContextTraitExt},
    window::Window,
    ContextWrapper, PossiblyCurrent,
};
use lazy_static::lazy_static;
use libmpv_sys::*;
use std::{
    ffi::{c_void, CStr, CString},
    mem::transmute,
    os::raw::c_char,
    ptr::{null, null_mut},
    rc::Rc,
    sync::Mutex,
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
    // use a rwlock for real
    println!("begin get_proc_addr {:?}", CStr::from_ptr(name));
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
        let evloop = glutin::event_loop::EventLoop::<MPVEvent>::with_user_event();
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
        (gl, "#version 140", window, evloop)
    };

    let mpv = unsafe {
        let mpv = mpv_create();
        assert!(!mpv.is_null(), "MPV failed to create!");
        mpv
    };

    let mut mpv_gl: *mut mpv_render_context = null_mut();
    unsafe {
        // NOTE, these sRGB parameters do not seem to work.
        // This is why we glDisable GL_FRAMEBUFFER_SRGB later, but I am not sure
        // if this is even the correct approach to make MPV output SRGB.
        let mut prop = "target-prim\0".to_owned();
        let c_prop = prop.as_mut_ptr() as *mut _;
        let mut val = "bt.709\0".to_owned();
        let c_val = val.as_mut_ptr() as *mut _;

        let ret = mpv_set_option_string(mpv, c_prop, c_val);
        println!("set prop string {}", mpv_error_str(ret));

        prop = "target-trc\0".to_owned();
        val = "srgb\0".to_owned();
        let c_prop = prop.as_mut_ptr() as *mut _;
        let c_val = val.as_mut_ptr() as *mut _;
        let ret = mpv_set_option_string(mpv, c_prop, c_val);
        println!("set prop string {}", mpv_error_str(ret));
        let mut ll = "debug\0".to_owned();
        let c_ll = ll.as_mut_ptr() as *mut _;
        mpv_request_log_messages(mpv, c_ll);

        assert!(mpv_initialize(mpv) == 0, "MPV failed to initialise!");
    };
    unsafe {
        println!("raw handle {:?}", window.context().raw_handle());
    }
    let q = "String".to_owned();

    unsafe {
        println!(
            "addr {:?}",
            transmute::<_, *mut c_void>(MPV_RENDER_API_TYPE_OPENGL)
        );
        println!("addr {:?}", transmute::<_, *mut c_void>(&window));
    }

    let proc_addr_addr = get_proc_addr as *const ();
    println!("addr {:?}", proc_addr_addr);

    let mpv_ogl_init_param = unsafe {
        mpv_opengl_init_params {
            get_proc_address: Some(get_proc_addr),
            get_proc_address_ctx: transmute(&window),
            extra_exts: null(),
        }
    };

    let mut mpv_render_params = unsafe {
        let proc_addr_addr = Some(get_proc_addr);
        println!("addr {:?}", &proc_addr_addr as *const _);
        let vv = vec![
            mpv_render_param {
                type_: mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                data: transmute(MPV_RENDER_API_TYPE_OPENGL),
            },
            mpv_render_param {
                type_: mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: transmute(&mpv_ogl_init_param),
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
        ];
        vv
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

    // Setup egui

    let gl = Rc::new(gl);

    let mut egui_glow = egui_glow::winit::EguiGlow::new(window.window(), gl.clone());
    let mut clear = [0.1, 0.1, 0.1];

    // https://github.com/grovesNL/glow/blob/main/examples/hello/src/main.rs
    evloop.run(move |event, _, ctrl_flow| {
        *ctrl_flow = ControlFlow::Wait;

        match event {
            Event::LoopDestroyed => {
                return;
            }
            Event::MainEventsCleared => window.window().request_redraw(),
            Event::RedrawRequested(_) => {
                let egui_repaint = egui_glow.run(window.window(), |egui_ctx| {
                    egui::Area::new("my_area")
                        .fixed_pos(egui::pos2(100.0, 100.0))
                        .show(egui_ctx, |ui| {
                            egui::Frame::none()
                                .fill(egui::Color32::LIGHT_BLUE)
                                .inner_margin(10.0)
                                .outer_margin(10.0)
                                .show(ui, |ui| {
                                    ui.heading("MPV Overlay");
                                    if ui.button("Quit").clicked() {
                                        println!("clicked quit");
                                        *ctrl_flow = ControlFlow::Exit;
                                    }
                                })
                        });
                });

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
                    egui_glow.paint(window.window());
                    gl.disable(glow::FRAMEBUFFER_SRGB);
                    gl.disable(glow::BLEND);
                }

                window.swap_buffers().unwrap();
            }
            Event::WindowEvent { window_id, event } => {
                match event {
                    WindowEvent::CloseRequested => unsafe {
                        mpv_render_context_free(mpv_gl);
                        mpv_detach_destroy(mpv);
                        *ctrl_flow = ControlFlow::Exit;
                    },
                    _ => {}
                }

                egui_glow.on_event(&event);
                window.window().request_redraw();
            }
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
                                let text: &mpv_event_log_message =
                                    unsafe { std::mem::transmute((*mpv_event).data) };
                                println!("mpv_log {}", unsafe {
                                    CStr::from_ptr(text.text).to_str().unwrap()
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
