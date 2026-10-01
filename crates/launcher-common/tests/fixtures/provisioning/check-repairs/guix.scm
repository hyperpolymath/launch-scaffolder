;; SPDX-License-Identifier: MPL-2.0
;; SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>
;;
;; guix.scm — docsr as a Guix package.
;;
;;   guix build -f guix.scm          # installs the source tree to share/docsr
;;   guix shell -D -f guix.scm       # the full development toolchain
;;
;; This is a SOURCE package, and says so: a hermetic compiled build needs this
;; repository's docs dependencies packaged in Guix, which they are not
;; (Guix builds offline). The toolchain below is real — `guix shell -D -f guix.scm`
;; then `just setup` gives a working environment. See docs/SETUP.adoc §Guix.
(use-modules (guix packages) (guix gexp)
             (guix build-system copy)
             (gnu packages)
             ((guix licenses) #:prefix license:))

;; The repository root: this file sits at the root or in build/ (PROVISIONING-STANDARD §1).
(define %source-dir
  (let ((d (dirname (current-filename))))
    (if (string=? (basename d) "build") (dirname d) d)))
;; A spec may name an output ("rust:cargo"); plain specification->package cannot.
(define (spec->input spec)
  (call-with-values (lambda () (specification->package+output spec))
    (lambda (pkg out) (if (string=? out "out") pkg (list pkg out)))))

(define %ignored
  '(".git" "target" ".eval" "node_modules" "_build" "deps" "zig-out" ".zig-cache" "dist-newstyle"))

(package
  (name "docsr")
  (version "0.1.0")
  (source (local-file %source-dir "docsr-checkout"
                      #:recursive? #t
                      #:select? (lambda (file stat)
                                  (not (member (basename file) %ignored)))))
  (build-system copy-build-system)
  (arguments (list #:install-plan #~'(("." "share/docsr/"))))
  (native-inputs (map spec->input (list "git" "bash" "coreutils" "nss-certs"
                                        "just" "mise" "shellcheck"
                                        "ruby-asciidoctor")))
  (home-page "https://github.com/hyperpolymath/docsr")
  (synopsis "docsr, a hyperpolymath repository")
  (description "docsr, a hyperpolymath repository.")
  (license license:mpl2.0))
