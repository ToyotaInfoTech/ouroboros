// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

/** Rule sets applied to every TypeScript source file of the WebUI. */
const recommendedRules = [
  js.configs.recommended,
  tseslint.configs.recommended,
  reactHooks.configs.flat.recommended,
  reactRefresh.configs.vite,
]

export default defineConfig([
  // Build output is generated and must not be linted.
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: recommendedRules,
    languageOptions: {
      ecmaVersion: 2020,
      globals: globals.browser,
    },
  },
])
