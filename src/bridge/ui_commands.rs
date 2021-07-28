use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossfire::mpsc::{unbounded_future, TxUnbounded, RxUnbounded};
use log::trace;
#[cfg(windows)]
use log::error;
use nvim_rs::Neovim;

use crate::bridge::TxWrapper;

#[cfg(windows)]
use crate::windows_utils::{
    register_rightclick_directory, register_rightclick_file, unregister_rightclick,
};

#[derive(Debug, Clone)]
pub enum UiCommand {
    Quit,
    Resize {
        width: u32,
        height: u32,
    },
    Keyboard(String),
    MouseButton {
        action: String,
        grid_id: u64,
        position: (u32, u32),
    },
    Scroll {
        direction: String,
        grid_id: u64,
        position: (u32, u32),
    },
    Drag {
        grid_id: u64,
        position: (u32, u32),
    },
    FileDrop(String),
    FocusLost,
    FocusGained,
    #[cfg(windows)]
    RegisterRightClick,
    #[cfg(windows)]
    UnregisterRightClick,
}

impl UiCommand {
    fn ok_to_drop(&self) -> bool {
        match self {
            UiCommand::Resize { .. } | 
            UiCommand::Scroll { .. } | 
            UiCommand::Drag { .. } => true,
            _ => false
        }
    }

    pub async fn execute(self, nvim: &Neovim<TxWrapper>) {
        match self {
            UiCommand::Quit => {
                nvim.command("qa!").await.ok();
            }
            UiCommand::Resize { width, height } => nvim
                .ui_try_resize(width.max(10) as i64, height.max(3) as i64)
                .await
                .expect("Resize failed"),
            UiCommand::Keyboard(input_command) => {
                trace!("Keyboard Input Sent: {}", input_command);
                nvim.input(&input_command).await.expect("Input failed");
            }
            UiCommand::MouseButton {
                action,
                grid_id,
                position: (grid_x, grid_y),
            } => {
                nvim.input_mouse(
                    "left",
                    &action,
                    "",
                    grid_id as i64,
                    grid_y as i64,
                    grid_x as i64,
                )
                .await
                .expect("Mouse Input Failed");
            }
            UiCommand::Scroll {
                direction,
                grid_id,
                position: (grid_x, grid_y),
            } => {
                nvim.input_mouse(
                    "wheel",
                    &direction,
                    "",
                    grid_id as i64,
                    grid_y as i64,
                    grid_x as i64,
                )
                .await
                .expect("Mouse Scroll Failed");
            }
            UiCommand::Drag {
                grid_id,
                position: (grid_x, grid_y),
            } => {
                nvim.input_mouse(
                    "left",
                    "drag",
                    "",
                    grid_id as i64,
                    grid_y as i64,
                    grid_x as i64,
                )
                .await
                .expect("Mouse Drag Failed");
            }
            UiCommand::FocusLost => nvim
                .command("if exists('#FocusLost') | doautocmd <nomodeline> FocusLost | endif")
                .await
                .expect("Focus Lost Failed"),
            UiCommand::FocusGained => nvim
                .command("if exists('#FocusGained') | doautocmd <nomodeline> FocusGained | endif")
                .await
                .expect("Focus Gained Failed"),
            UiCommand::FileDrop(path) => {
                nvim.command(format!("e {}", path).as_str()).await.ok();
            }
            #[cfg(windows)]
            UiCommand::RegisterRightClick => {
                if unregister_rightclick() {
                    let msg = "Could not unregister previous menu item. Possibly already registered or not running as Admin?";
                    nvim.err_writeln(msg).await.ok();
                    error!("{}", msg);
                }
                if !register_rightclick_directory() {
                    let msg = "Could not register directory context menu item. Possibly already registered or not running as Admin?";
                    nvim.err_writeln(msg).await.ok();
                    error!("{}", msg);
                }
                if !register_rightclick_file() {
                    let msg = "Could not register file context menu item. Possibly already registered or not running as Admin?";
                    nvim.err_writeln(msg).await.ok();
                    error!("{}", msg);
                }
            }
            #[cfg(windows)]
            UiCommand::UnregisterRightClick => {
                if !unregister_rightclick() {
                    let msg = "Could not remove context menu items. Possibly already removed or not running as Admin?";
                    nvim.err_writeln(msg).await.ok();
                    error!("{}", msg);
                }
            }
        }
    }
}

pub fn start_command_processors(ui_command_receiver: RxUnbounded<UiCommand>, running: Arc<AtomicBool>, nvim: Arc<Neovim<TxWrapper>>) {
    let (droppable_sender, droppable_receiver) = unbounded_future::<UiCommand>();
    let (non_droppable_sender, non_droppable_receiver) = unbounded_future::<UiCommand>();

    let droppable_nvim = nvim.clone();
    let droppable_running = running.clone();
    tokio::spawn(async move {
        loop {
            if !droppable_running.load(Ordering::Relaxed) {
                break;
            }

            let mut latest = droppable_receiver.recv().await.expect("Could not recieve droppable ui command");
            while let Ok(new_latest) = droppable_receiver.try_recv() {
                latest = new_latest;
            }

            let nvim = droppable_nvim.clone();
            tokio::spawn(async move {
                latest.execute(&nvim).await;
            });
        }
    });

    let non_droppable_nvim = nvim.clone();
    let non_droppable_running = running.clone();
    tokio::spawn(async move {
        loop {
            if !non_droppable_running.load(Ordering::Relaxed) {
                break;
            }

            match non_droppable_receiver.recv().await {
                Ok(non_droppable_ui_command) => {
                    non_droppable_ui_command.execute(&non_droppable_nvim).await;
                },
                Err(_) => {
                    non_droppable_running.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
    });

    tokio::spawn(async move {
        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            match ui_command_receiver.recv().await {
                Ok(ui_command) => {
                    if ui_command.ok_to_drop() {
                        droppable_sender.send(ui_command).expect("Could not send droppable command");
                    } else {
                        non_droppable_sender.send(ui_command).expect("Could not send non droppable command");
                    }
                }
                Err(_) => {
                    running.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
    });
}
