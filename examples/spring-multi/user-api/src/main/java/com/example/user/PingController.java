package com.example.user;

import java.util.LinkedHashMap;
import java.util.Map;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;
import jakarta.servlet.http.HttpServletRequest;

@RestController
@RequestMapping("/api/user")
public class PingController {

    @Value("${server.port:8080}")
    private String port;

    /** 回显转发头：经网关反代时可直接核对 Host / X-Forwarded-For / X-Real-IP 是否正确透传。 */
    @GetMapping("/ping")
    public Map<String, Object> ping(HttpServletRequest request) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("service", "user-api");
        out.put("port", port);
        out.put("host", request.getHeader("Host"));
        out.put("x-forwarded-for", request.getHeader("X-Forwarded-For"));
        out.put("x-forwarded-proto", request.getHeader("X-Forwarded-Proto"));
        out.put("x-real-ip", request.getHeader("X-Real-IP"));
        out.put("remote", request.getRemoteAddr());
        return out;
    }
}
