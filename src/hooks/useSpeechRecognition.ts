import { useCallback, useEffect, useRef, useState } from 'react';

// SpeechRecognition interface declarations for TypeScript
interface IWindow extends Window {
  SpeechRecognition?: any; // eslint-disable-line @typescript-eslint/no-explicit-any
  webkitSpeechRecognition?: any; // eslint-disable-line @typescript-eslint/no-explicit-any
}

interface UseSpeechRecognitionOptions {
  onTranscript?: (text: string) => void;
  lang?: string;
}

export function useSpeechRecognition({ onTranscript, lang }: UseSpeechRecognitionOptions = {}) {
  const [isListening, setIsListening] = useState(false);
  const [isSupported, setIsSupported] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recognitionRef = useRef<any>(null); // eslint-disable-line @typescript-eslint/no-explicit-any
  const onTranscriptRef = useRef(onTranscript);
  onTranscriptRef.current = onTranscript;

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const win = window as IWindow;
    const SpeechRecognitionClass = win.SpeechRecognition || win.webkitSpeechRecognition;

    if (SpeechRecognitionClass) {
      setIsSupported(true);
      try {
        const recognition = new SpeechRecognitionClass();
        recognition.continuous = true;
        recognition.interimResults = true;
        recognition.lang = lang || (typeof navigator !== 'undefined' ? navigator.language : 'en-US');

        recognition.onresult = (event: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          // Only finalized results are forwarded. Interim results change on
          // every audio chunk; emitting them would append duplicate text to
          // the composer on each update.
          for (let i = event.resultIndex; i < event.results.length; i++) {
            const result = event.results[i];
            if (!result.isFinal) continue;
            const transcript = result[0]?.transcript?.trim();
            if (transcript && onTranscriptRef.current) {
              onTranscriptRef.current(transcript);
            }
          }
        };

        recognition.onerror = (event: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          console.warn('[SpeechRecognition] error:', event.error);
          setError(event.error);
          setIsListening(false);
        };

        recognition.onend = () => {
          setIsListening(false);
        };

        recognitionRef.current = recognition;
      } catch (err) {
        console.warn('[SpeechRecognition] Init failed:', err);
        setIsSupported(false);
      }
    } else {
      setIsSupported(false);
    }

    return () => {
      if (recognitionRef.current) {
        try {
          recognitionRef.current.abort();
        } catch {
          // ignore
        }
      }
    };
  }, [lang]);

  const startListening = useCallback(() => {
    if (!recognitionRef.current || isListening) return;
    setError(null);
    try {
      recognitionRef.current.start();
      setIsListening(true);
    } catch (err) {
      console.warn('[SpeechRecognition] start error:', err);
      setError(String(err));
    }
  }, [isListening]);

  const stopListening = useCallback(() => {
    if (!recognitionRef.current || !isListening) return;
    try {
      recognitionRef.current.stop();
    } catch {
      // ignore
    }
    setIsListening(false);
  }, [isListening]);

  const toggleListening = useCallback(() => {
    if (isListening) {
      stopListening();
    } else {
      startListening();
    }
  }, [isListening, startListening, stopListening]);

  return {
    isListening,
    isSupported,
    error,
    startListening,
    stopListening,
    toggleListening,
  };
}
