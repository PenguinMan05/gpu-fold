use super::*;
use repair_core::types::{DialogueEntry,DialogueRole};

impl App {
    fn conversation_entries(&self) -> Vec<DialogueEntry> {
        let live=self.engine.conversation();
        if !live.is_empty() { return live; }
        self.run.as_ref().map(|run|run.dialogue.clone()).unwrap_or_default()
    }
    pub(super) fn task_controls(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("task_controls").exact_width(184.0).resizable(false).show(ctx, |ui| {
            ui.add_space(16.0);
            ui.small("TASK");
            ui.add_space(8.0);
            if self.pending.is_some() {
                ui.horizontal(|ui| { ui.spinner(); ui.strong("Working"); });
            } else { ui.strong("Ready"); }
            ui.add_space(10.0);
            if ui.add_sized([160.0,40.0],egui::Button::new("AI conversation")).clicked() { self.show_conversation=!self.show_conversation; }
            if self.pending.is_some() && ui.add_sized([160.0,36.0],egui::Button::new("Stop current task")).clicked() {
                self.cancel.store(true,Ordering::SeqCst);
                self.message="Stopping… Your attempts are saved in History.".into();
            }
            ui.add_space(12.0);
            egui::ScrollArea::vertical().id_salt("task_status_message").max_height(180.0).show(ui,|ui| { ui.add(egui::Label::new(&self.message).wrap()); });
            ui.add_space(10.0);
            ui.small("Ctrl+4 opens the conversation.");
        });
    }
    pub(super) fn conversation_body(&mut self, ui: &mut egui::Ui) {
        let entries=self.conversation_entries();
        egui::ScrollArea::vertical().id_salt("conversation_feed").stick_to_bottom(true).auto_shrink([false,false]).show(ui,|ui| {
            ui.set_width(ui.available_width());
            if entries.is_empty() {
                ui.add_space(20.0);
                ui.heading("Your AI workspace");
                ui.label("When a task starts, its investigation, source reads, findings, revisions, and proposed repair appear here.");
            }
            for (index,entry) in entries.iter().enumerate() {
                let (label,color)=match entry.role {
                    DialogueRole::Task => ("Task",egui::Color32::from_rgb(237,239,244)),
                    DialogueRole::Assistant => ("GPUFold",egui::Color32::from_rgb(250,248,245)),
                    DialogueRole::Activity => ("Activity",egui::Color32::from_rgb(245,246,248)),
                    DialogueRole::Result => ("Result",egui::Color32::from_rgb(234,245,239)),
                };
                ui.push_id(index,|ui| {
                    egui::Frame::none().fill(color).rounding(12.0).inner_margin(16.0).show(ui,|ui| {
                        ui.set_width((ui.available_width()-32.0).max(120.0));
                        ui.strong(label);
                        ui.add_space(5.0);
                        ui.add(egui::Label::new(&entry.text).wrap().selectable(true));
                    });
                    ui.add_space(12.0);
                });
            }
            if self.pending.is_some() {
                let partial=self.engine.model_output.lock().map(|raw| repair_core::model::proposal_summary(&raw)).unwrap_or_default();
                let already_shown=entries.iter().rev().find(|entry|entry.role==DialogueRole::Assistant).is_some_and(|entry|entry.text==partial);
                if !partial.is_empty() && !already_shown {
                    egui::Frame::none().fill(egui::Color32::from_rgb(250,248,245)).rounding(12.0).inner_margin(16.0).show(ui,|ui| {
                        ui.strong("GPUFold · replying");
                        ui.add(egui::Label::new(process::scrub(&partial)).wrap().selectable(true));
                    });
                }
                ui.horizontal(|ui| { ui.spinner(); ui.small("Investigating and preparing the next response…"); });
            }
        });
    }
    pub(super) fn conversation_window(&mut self, ctx: &egui::Context) {
        if !self.show_conversation { return; }
        let mut open=true;
        egui::Window::new("AI conversation").id(egui::Id::new("ai_conversation_window")).open(&mut open)
            .default_size([650.0,560.0]).min_size([360.0,260.0]).resizable(true).collapsible(false)
            .show(ctx,|ui| self.conversation_body(ui));
        self.show_conversation=open;
    }
}
