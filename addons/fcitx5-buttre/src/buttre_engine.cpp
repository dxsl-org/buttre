/* fcitx5-buttre — the C++ shim between fcitx5's in-process engine API and
 * buttre's Rust core (buttre_ffi.h). Kept deliberately thin: ALL composition
 * semantics live behind bt_engine_process_keysym (the same EngineBridge the
 * IBus and Wayland paths use), so the three Linux paths cannot drift.
 *
 * Tri-surface sync: the shared method file (~/.config/buttre/method) is the
 * source of truth for the active method. It is re-checked (one stat) per
 * key event and on activate — a switch made in the tray or config window
 * applies on the next keystroke, mirroring the IBus engine's per-keystroke
 * generation check. This addon never WRITES the file (fcitx5 v1 registers a
 * single "Buttre" input method; there is no per-method radio to click).
 */

#include <fcitx-utils/utf8.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>

#include <buttre_ffi.h>

#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <string>

namespace {

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

} // namespace

class ButtreEngine final : public fcitx::InputMethodEngineV2 {
public:
    explicit ButtreEngine(fcitx::Instance *instance) : instance_(instance) {
        engine_ = bt_engine_new(nullptr);
        refreshMethodFromSharedFile();
    }

    ~ButtreEngine() override { bt_engine_free(engine_); }

    void keyEvent(const fcitx::InputMethodEntry & /*entry*/,
                  fcitx::KeyEvent &keyEvent) override {
        if (keyEvent.isRelease() || engine_ == 0) {
            return;
        }
        refreshMethodFromSharedFile();
        /* Ctrl/Alt/Super combos (copy, paste, save, window shortcuts) carry
         * no modifier info through bt_engine_process_keysym (keysym-only
         * ABI), so process_keysym would compose the bare letter and this
         * addon would swallow every shortcut. Commit whatever is pending —
         * same rule the ibus/wayland backends apply
         * (ibus.rs::is_control_combo) — then let the combo reach the
         * client untouched. */
        if (keyEvent.rawKey().states().testAny(fcitx::KeyState::Ctrl_Alt_Super)) {
            applyResult(keyEvent.inputContext(), bt_engine_flush(engine_));
            return;
        }
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
                  fcitx::InputContextEvent & /*event*/) override {
        refreshMethodFromSharedFile();
        /* One engine_ handle serves every input context (fcitx5 creates one
         * engine instance per addon, not per IC) — without this, a
         * composition left pending when a window closed without
         * deactivate() (or reset() not delivered) would leak into the next
         * focused window's first keystroke. */
        bt_engine_reset(engine_);
    }

    /* Focus is leaving: commit the pending word (never drop typed text). */
    void deactivate(const fcitx::InputMethodEntry & /*entry*/,
                    fcitx::InputContextEvent &event) override {
        if (engine_ == 0) {
            return;
        }
        applyResult(event.inputContext(), bt_engine_flush(engine_));
    }

    /* Hard reset (Escape at the fcitx level, IC reset): discard, no commit. */
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
            list->append<ButtreCandidateWord>(const_cast<ButtreEngine *>(this), i,
                                              fcitx::Text(display));
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

    /* One stat per call; reload only on mtime change. Unknown/empty file
     * falls back to telex — same normalization the Rust reader applies. */
    void refreshMethodFromSharedFile() {
        const auto path = sharedMethodPath();
        if (path.empty()) {
            return;
        }
        std::error_code ec;
        const auto mtime = std::filesystem::last_write_time(path, ec);
        if (ec || mtime == methodMtime_) {
            return;
        }
        methodMtime_ = mtime;
        std::string method = readTrimmed(path);
        if (method.empty()) {
            method = "telex";
        }
        if (method != method_ && bt_engine_set_method(engine_, method.c_str())) {
            method_ = method;
        }
    }

    fcitx::Instance *instance_;
    uint64_t engine_ = 0;
    std::string method_ = "telex";
    std::filesystem::file_time_type methodMtime_{};
};

class ButtreEngineFactory final : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new ButtreEngine(manager->instance());
    }
};

FCITX_ADDON_FACTORY(ButtreEngineFactory)
