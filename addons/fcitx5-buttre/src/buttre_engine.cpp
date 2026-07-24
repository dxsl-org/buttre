/* fcitx5-buttre — the C++ shim between fcitx5's in-process engine API and
 * buttre's Rust core (buttre_ffi.h). Kept deliberately thin: ALL composition
 * semantics live behind bt_engine_process_keysym (the same EngineBridge the
 * IBus and Wayland paths use), so the three Linux paths cannot drift.
 *
 * Tri-surface sync — BOTH directions:
 *  - read: the shared method file (~/.config/buttre/method) is re-checked
 *    (one stat) per key event and on activate, mirroring the IBus engine's
 *    per-keystroke generation check — a switch made in the tray or config
 *    window applies on the next keystroke and re-checks the panel menu.
 *  - write: picking a method from this addon's status-area menu writes the
 *    same file (atomic temp+rename, mirroring method_sync::write_method_to)
 *    so the tray and config window follow, exactly like an IBus-panel radio
 *    click.
 */

#include <fcitx-utils/misc.h>
#include <fcitx/action.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx/menu.h>
#include <fcitx/statusarea.h>
#include <fcitx/userinterfacemanager.h>

#include <buttre_ffi.h>

#include <array>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <memory>
#include <string>

namespace {

/* The four built-in methods, in tray-menu order. Custom keyboard TOMLs are
 * accepted through the shared file (read side) but not listed here yet. */
struct MethodEntry {
    const char *id;
    const char *label;
};
constexpr std::array<MethodEntry, 4> kMethods{{
    {"english", "English"},
    {"telex", "Telex"},
    {"vni", "VNI"},
    {"nom", "Chữ Nôm"},
}};

/* ~/.config/buttre/method (XDG_CONFIG_HOME honored, like dirs::config_dir). */
std::filesystem::path sharedMethodPath() {
    if (const char *xdg = std::getenv("XDG_CONFIG_HOME"); xdg && *xdg) {
        return std::filesystem::path(xdg) / "buttre/method";
    }
    if (const char *home = std::getenv("HOME"); home && *home) {
        return std::filesystem::path(home) / ".config/buttre/method";
    }
    return {};
}

std::string readTrimmed(const std::filesystem::path &path) {
    std::ifstream in(path);
    std::string content((std::istreambuf_iterator<char>(in)),
                        std::istreambuf_iterator<char>());
    const auto last = content.find_last_not_of(" \t\r\n");
    return last == std::string::npos ? std::string() : content.substr(0, last + 1);
}

/* Atomic write, mirroring the Rust side (method_sync::write_method_to):
 * temp file + rename in the same directory so no reader sees a torn file. */
bool writeSharedMethod(const std::string &method) {
    const auto path = sharedMethodPath();
    if (path.empty()) {
        return false;
    }
    std::error_code ec;
    std::filesystem::create_directories(path.parent_path(), ec);
    if (ec) {
        return false;
    }
    const auto tmp = path.parent_path() / ".method.tmp";
    {
        std::ofstream out(tmp, std::ios::trunc);
        if (!out) {
            return false;
        }
        out << method;
    }
    std::filesystem::rename(tmp, path, ec);
    return !ec;
}

std::string labelFor(const std::string &method) {
    for (const auto &entry : kMethods) {
        if (method == entry.id) {
            return entry.label;
        }
    }
    return method; /* custom keyboard id — show it verbatim */
}

} // namespace

class ButtreEngine final : public fcitx::InputMethodEngineV2 {
public:
    explicit ButtreEngine(fcitx::Instance *instance) : instance_(instance) {
        engine_ = bt_engine_new(nullptr);
        setupActions();
        refreshMethodFromSharedFile(nullptr);
    }

    ~ButtreEngine() override { bt_engine_free(engine_); }

    void keyEvent(const fcitx::InputMethodEntry & /*entry*/,
                  fcitx::KeyEvent &keyEvent) override {
        if (keyEvent.isRelease() || engine_ == 0) {
            return;
        }
        refreshMethodFromSharedFile(keyEvent.inputContext());
        const BtKeyResult result = bt_engine_process_keysym(
            engine_, static_cast<uint32_t>(keyEvent.rawKey().sym()));
        applyResult(keyEvent.inputContext(), result);
        if (result.handled) {
            keyEvent.filterAndAccept();
        }
        /* handled == false with a commit set is the break-key contract:
         * the word was committed above and the original key still reaches
         * the client because we did NOT filter the event. */
    }

    void activate(const fcitx::InputMethodEntry & /*entry*/,
                  fcitx::InputContextEvent &event) override {
        auto *ic = event.inputContext();
        refreshMethodFromSharedFile(ic);
        /* (Re)attach the panel menu for this input context — the status
         * area is per-IC and cleared when the input method changes. */
        auto &statusArea = ic->statusArea();
        statusArea.addAction(fcitx::StatusGroup::InputMethod, &methodRootAction_);
        statusArea.addAction(fcitx::StatusGroup::InputMethod, &configAction_);
    }

    /* Focus is leaving: commit the pending word (never drop typed text). */
    void deactivate(const fcitx::InputMethodEntry & /*entry*/,
                    fcitx::InputContextEvent &event) override {
        if (engine_ == 0) {
            return;
        }
        applyResult(event.inputContext(), bt_engine_flush(engine_));
    }

    /* Hard reset (IC reset): discard, no commit. */
    void reset(const fcitx::InputMethodEntry & /*entry*/,
               fcitx::InputContextEvent &event) override {
        if (engine_ == 0) {
            return;
        }
        bt_engine_reset(engine_);
        clearPanel(event.inputContext());
    }

    /* Candidate click/selection → committed value + fresh panel state. */
    void selectCandidate(fcitx::InputContext *ic, uint32_t index) {
        applyResult(ic, bt_engine_select_candidate(engine_, index));
    }

private:
    class ButtreCandidateWord final : public fcitx::CandidateWord {
    public:
        ButtreCandidateWord(ButtreEngine *engine, uint32_t index, fcitx::Text text)
            : fcitx::CandidateWord(std::move(text)), engine_(engine), index_(index) {}
        void select(fcitx::InputContext *ic) const override {
            engine_->selectCandidate(ic, index_);
        }

    private:
        ButtreEngine *engine_;
        uint32_t index_;
    };

    /* Build the status-area UI once: a "Kiểu gõ" root action carrying the
     * method menu (one checkable action per built-in method — fcitx has no
     * radio group primitive, checked state is maintained by hand in
     * syncActionStates), plus a "Cấu hình…" launcher. */
    void setupActions() {
        auto &ui = instance_->userInterfaceManager();
        methodRootAction_.setLongText("Kiểu gõ (buttre)");
        ui.registerAction("buttre-method", &methodRootAction_);
        methodRootAction_.setMenu(&methodMenu_);
        for (const auto &entry : kMethods) {
            auto action = std::make_unique<fcitx::SimpleAction>();
            action->setShortText(entry.label);
            action->setCheckable(true);
            const std::string id = entry.id;
            connections_.emplace_back(action->connect<fcitx::SimpleAction::Activated>(
                [this, id](fcitx::InputContext *ic) { switchMethod(id, ic); }));
            ui.registerAction(std::string("buttre-method-") + entry.id, action.get());
            methodMenu_.addAction(action.get());
            methodActions_.push_back(std::move(action));
        }
        configAction_.setShortText("Cấu hình…");
        configAction_.setLongText("Mở cửa sổ cấu hình buttre");
        connections_.emplace_back(configAction_.connect<fcitx::SimpleAction::Activated>(
            [](fcitx::InputContext * /*ic*/) {
                fcitx::startProcess({"buttre", "--config"});
            }));
        ui.registerAction("buttre-config", &configAction_);
    }

    /* Panel-menu click: switch the engine AND persist to the shared file so
     * the tray and config window follow (the write half of tri-surface). */
    void switchMethod(const std::string &id, fcitx::InputContext *ic) {
        if (id == method_ || !bt_engine_set_method(engine_, id.c_str())) {
            return;
        }
        method_ = id;
        if (writeSharedMethod(id)) {
            /* Remember the mtime WE created so the per-keystroke check does
             * not immediately re-read our own write (echo suppression). */
            std::error_code ec;
            const auto mtime = std::filesystem::last_write_time(sharedMethodPath(), ec);
            if (!ec) {
                methodMtime_ = mtime;
            }
        }
        syncActionStates(ic);
        clearPanel(ic); /* a method switch resets any live composition */
    }

    /* Map one ABI result onto the input context. Order is the bridge's
     * contract: commit FIRST (the committed word must land before any
     * preedit/panel change), then preedit, then candidates. */
    void applyResult(fcitx::InputContext *ic, const BtKeyResult &result) {
        if (ic == nullptr) {
            return;
        }
        if (result.commit != nullptr && *result.commit != '\0') {
            ic->commitString(result.commit);
        }
        const std::string preedit = result.preedit ? result.preedit : "";
        fcitx::Text text(preedit, fcitx::TextFormatFlag::Underline);
        text.setCursor(static_cast<int>(preedit.size()));
        if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Preedit)) {
            ic->inputPanel().setClientPreedit(text);
        } else {
            ic->inputPanel().setPreedit(text);
        }
        updateCandidates(ic);
        ic->updatePreedit();
        ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    }

    void updateCandidates(fcitx::InputContext *ic) {
        const uint32_t count = bt_engine_candidate_count(engine_);
        if (count == 0) {
            ic->inputPanel().setCandidateList(nullptr);
            return;
        }
        auto list = std::make_unique<fcitx::CommonCandidateList>();
        list->setPageSize(9);
        for (uint32_t i = 0; i < count; ++i) {
            const char *display = bt_engine_candidate_display(engine_, i);
            if (display == nullptr) {
                break; /* list changed under us — stop at the ABI's edge */
            }
            list->append<ButtreCandidateWord>(this, i, fcitx::Text(display));
        }
        ic->inputPanel().setCandidateList(std::move(list));
    }

    void clearPanel(fcitx::InputContext *ic) {
        if (ic == nullptr) {
            return;
        }
        ic->inputPanel().reset();
        ic->updatePreedit();
        ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    }

    /* Push method_ into the menu UI: root shows the current method, exactly
     * one item is checked. `ic` non-null → repaint that context's panel. */
    void syncActionStates(fcitx::InputContext *ic) {
        methodRootAction_.setShortText("Kiểu gõ: " + labelFor(method_));
        for (size_t i = 0; i < methodActions_.size(); ++i) {
            methodActions_[i]->setChecked(method_ == kMethods[i].id);
        }
        if (ic != nullptr) {
            methodRootAction_.update(ic);
            for (auto &action : methodActions_) {
                action->update(ic);
            }
        }
    }

    /* One stat per call; reload only on mtime change. Unknown/empty file
     * falls back to telex — same normalization the Rust reader applies. */
    void refreshMethodFromSharedFile(fcitx::InputContext *ic) {
        const auto path = sharedMethodPath();
        if (path.empty()) {
            return;
        }
        std::error_code ec;
        const auto mtime = std::filesystem::last_write_time(path, ec);
        if (!ec && mtime == methodMtime_) {
            return;
        }
        if (!ec) {
            methodMtime_ = mtime;
        }
        std::string method = readTrimmed(path);
        if (method.empty()) {
            method = "telex";
        }
        if (method != method_ && bt_engine_set_method(engine_, method.c_str())) {
            method_ = method;
        }
        syncActionStates(ic);
    }

    fcitx::Instance *instance_;
    uint64_t engine_ = 0;
    std::string method_ = "telex";
    std::filesystem::file_time_type methodMtime_{};

    fcitx::SimpleAction methodRootAction_;
    fcitx::SimpleAction configAction_;
    fcitx::Menu methodMenu_;
    std::vector<std::unique_ptr<fcitx::SimpleAction>> methodActions_;
    std::vector<fcitx::ScopedConnection> connections_;
};

class ButtreEngineFactory final : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new ButtreEngine(manager->instance());
    }
};

FCITX_ADDON_FACTORY(ButtreEngineFactory)
