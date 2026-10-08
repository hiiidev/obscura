//! Host-side processing for website-created popup windows.
//! JS only queues opaque handles. The CDP owner supplies Context, opener and
//! authentication; untrusted scripts never name the Context themselves.
use obscura_browser::Page;
use obscura_js::ops::WindowAction;
use serde_json::{json, Value};

use crate::dispatch::CdpContext;
use crate::types::CdpEvent;

/// Build consistent TargetInfo for both normal targets and website-created
/// windows. An about:blank popup inherits the opener's origin until navigation.
pub(crate) fn target_info(ctx: &CdpContext, page: &Page, attached: bool) -> Value {
    let opener = page.opener_id.as_deref();
    let can_access_opener = opener.and_then(|id| ctx.get_page(id)).is_some_and(|opener_page| {
        if page.url_string() == "about:blank" {
            return true;
        }
        let lhs = url::Url::parse(&page.url_string()).ok();
        let rhs = url::Url::parse(&opener_page.url_string()).ok();
        matches!((lhs, rhs), (Some(a), Some(b)) if a.origin() == b.origin())
    });
    let mut info = json!({
        "targetId": page.id,
        "type": "page",
        "title": page.title,
        "url": page.url_string(),
        "attached": attached,
        "canAccessOpener": can_access_opener,
        "browserContextId": page.context.id,
    });
    if let Some(opener) = opener {
        info["openerId"] = json!(opener);
    }
    if let Some(frame) = page.opener_frame_id.as_deref() {
        info["openerFrameId"] = json!(frame);
    }
    info
}

fn close_popup(ctx: &mut CdpContext, target_id: &str) {
    let sessions: Vec<String> = ctx.sessions.iter()
        .filter_map(|(sid, page)| (page == target_id).then_some(sid.clone()))
        .collect();
    for sid in sessions {
        ctx.pending_events.push(CdpEvent::new(
            "Target.detachedFromTarget",
            json!({"sessionId":sid,"targetId":target_id}),
        ));
    }
    ctx.pending_events.push(CdpEvent::new(
        "Target.targetDestroyed", json!({"targetId":target_id}),
    ));
    ctx.remove_page(target_id);
}

/// No V8 entry and no network I/O here: it is safe to call this directly
/// after a CDP command and after an autonomous event-loop turn. Navigation is
/// dispatched later, after targetCreated/attachedToTarget have been forwarded.
pub(crate) fn drain_window_actions(ctx: &mut CdpContext) {
    let actions: Vec<(String, WindowAction)> = ctx.pages.iter()
        .flat_map(|p| p.take_pending_window_actions().into_iter()
            .map(|act| (p.id.clone(), act)))
        .collect();

    for (source_id, action) in actions {
        match action {
            WindowAction::Open { handle, url, frame_id, noopener } => {
                let Some(source) = ctx.get_page(&source_id) else { continue };
                let context_id = source.context.id.clone();
                let file_allowed = source.context.allow_file_access;
                if crate::util::url_is_file_scheme(&url) && !file_allowed {
                    source.mark_window_closed(handle);
                    continue;
                }
                let opener_frame_id = if frame_id == 0 {
                    source.frame_id.clone()
                } else {
                    format!("{}-frame-{}", source.frame_id, frame_id)
                };
                let session_storage = if noopener {
                    None
                } else {
                    ctx.get_page_mut(&source_id)
                        .map(Page::popup_session_storage_snapshot)
                };
                let Ok(target_id) = ctx.create_page_in_context(Some(&context_id)) else {
                    if let Some(source) = ctx.get_page(&source_id) {
                        source.mark_window_closed(handle);
                    }
                    continue;
                };
                if let Some(page) = ctx.get_page_mut(&target_id) {
                    if !noopener {
                        page.opener_id = Some(source_id.clone());
                        page.opener_frame_id = Some(opener_frame_id);
                    }
                    if let Some(storage) = session_storage {
                        page.inherit_popup_session_storage(storage);
                    }
                }
                ctx.popup_targets.insert((source_id.clone(), handle), target_id.clone());

                for (sid, owner) in &ctx.sessions {
                    if owner == &source_id {
                        ctx.pending_events.push(CdpEvent::with_session(
                            "Page.windowOpen",
                            json!({
                                "url": url,
                                "windowName": "_blank",
                                "windowFeatures": [],
                                "userGesture": false,
                            }),
                            sid.clone(),
                        ));
                    }
                }

                if let Some(page) = ctx.get_page(&target_id) {
                    let info = target_info(ctx, page, false);
                    ctx.pending_events.push(CdpEvent::new(
                        "Target.targetCreated", json!({"targetInfo":info}),
                    ));
                }

                // Always allocate one internal navigation route, even when a
                // client has not requested auto-attach.
                let route_session = ctx.next_target_session(&target_id);
                ctx.sessions.insert(route_session.clone(), target_id.clone());
                let subscriptions: Vec<_> = ctx.auto_attach_options.iter()
                    .filter(|(_, opt)| opt.enabled)
                    .map(|(sid, opt)| (sid.clone(), *opt))
                    .collect();
                let mut paused = false;
                for (parent_sid, options) in subscriptions {
                    let child_session = ctx.next_target_session(&target_id);
                    ctx.sessions.insert(child_session.clone(), target_id.clone());
                    let Some(page) = ctx.get_page(&target_id) else { continue };
                    let info = target_info(ctx, page, true);
                    let payload = json!({
                        "sessionId": child_session,
                        "targetInfo": info,
                        "waitingForDebugger": options.wait_for_debugger,
                    });
                    let event = match parent_sid {
                        Some(parent) => CdpEvent::with_session("Target.attachedToTarget", payload, parent),
                        None => CdpEvent::new("Target.attachedToTarget", payload),
                    };
                    ctx.pending_events.push(event);
                    if options.wait_for_debugger {
                        paused = true;
                        ctx.popup_paused.insert(child_session, (target_id.clone(), url.clone()));
                    }
                }
                if !paused && url != "about:blank" {
                    ctx.popup_nav_queue.push_back((route_session, url));
                }
            }
            WindowAction::Close { handle } => {
                let key = (source_id, handle);
                if let Some(target_id) = ctx.popup_targets.get(&key).cloned() {
                    close_popup(ctx, &target_id);
                }
            }
            WindowAction::Navigate { handle, url } => {
                if let Some(target_id) = ctx.popup_targets.get(&(source_id, handle)).cloned() {
                    if let Some(session_id) = ctx.sessions.iter()
                        .find_map(|(sid, page)| (page == &target_id).then_some(sid.clone()))
                    {
                        ctx.popup_nav_queue.push_back((session_id, url));
                    }
                }
            }
        }
    }
}

/// Returns queued synthetic CDP Page.navigate arguments, only after popup
/// target creation events have been forwarded to the client.
pub(crate) fn take_navigation(ctx: &mut CdpContext) -> Option<(String, String)> {
    ctx.popup_nav_queue.pop_front()
}

/// Resume a truly paused popup. The URL has not been loaded, and no author
/// script has run in that target before Runtime.runIfWaitingForDebugger.
pub(crate) fn resume(ctx: &mut CdpContext, session_id: &Option<String>) {
    let Some(sid) = session_id.as_deref() else { return };
    let Some((target, url)) = ctx.popup_paused.remove(sid) else { return };
    if !ctx.popup_paused.values().any(|(other, _)| other == &target)
        && url != "about:blank"
    {
        ctx.popup_nav_queue.push_back((sid.to_string(), url));
    }
}
